//! Analog engine — "what past period looks like now," with a forward-return
//! *distribution*, not a point estimate (`docs/ENVIRONMENT-ENGINE.md` §3.2).
//!
//! Distance is **Mahalanobis** (covariance-whitened) over the environment
//! feature vector — the right metric for correlated financial features. The k
//! nearest historical days are the analogs; the distribution of the target
//! asset's realized forward returns following those days is the probabilistic
//! forecast, with the nearest-neighbour distance reported as analog quality and
//! a block-bootstrap CI on the median.
//!
//! Pure Rust: a small Gauss-Jordan inverse handles the (d≈12) covariance —
//! no linear-algebra dependency. All values are `f64`.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate};
use serde::Serialize;

use super::environment::EnvironmentSeries;
use crate::research::validation;

/// Ridge added to the covariance diagonal for numerical conditioning.
const RIDGE: f64 = 1e-3;

#[derive(Debug, Clone, Serialize)]
pub struct AnalogMatch {
    pub date: String,
    /// Mahalanobis distance from the query environment (smaller = closer).
    pub distance: f64,
    /// Growth×inflation regime quad on the analog date.
    pub regime: String,
    /// Target asset's forward return from this analog date (%), if resolvable.
    pub forward_return_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalogReport {
    pub query_date: String,
    /// Growth×inflation regime quad today.
    pub query_regime: String,
    pub target_asset: String,
    pub horizon_days: i64,
    pub k: usize,
    /// Analogs actually used in the forward-return stats (target data present).
    pub n_with_forward: usize,
    pub analogs: Vec<AnalogMatch>,
    /// Mean Mahalanobis distance of the k analogs — the lower, the more like now.
    pub mean_distance: f64,
    pub median_forward_pct: Option<f64>,
    pub mean_forward_pct: Option<f64>,
    pub p25_forward_pct: Option<f64>,
    pub p75_forward_pct: Option<f64>,
    /// Share of analogs whose forward return was positive.
    pub up_rate_pct: Option<f64>,
    /// Block-bootstrap 90% CI on the mean forward return (%).
    pub mean_forward_ci_pct: Option<(f64, f64)>,
    /// Honest quality note (e.g. thin analog coverage for a young target).
    pub note: String,
}

/// Invert a symmetric matrix via Gauss-Jordan with partial pivoting.
#[allow(clippy::needless_range_loop)] // genuine 2D matrix indexing
fn invert(mat: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = mat.len();
    if n == 0 || mat.iter().any(|r| r.len() != n) {
        return None;
    }
    // Augment [A | I].
    let mut a: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let mut row = mat[i].clone();
            row.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
            row
        })
        .collect();
    for col in 0..n {
        // Partial pivot.
        let pivot = (col..n).max_by(|&r1, &r2| a[r1][col].abs().partial_cmp(&a[r2][col].abs()).unwrap())?;
        if a[pivot][col].abs() < 1e-15 {
            return None;
        }
        a.swap(col, pivot);
        let div = a[col][col];
        for x in a[col].iter_mut() {
            *x /= div;
        }
        for r in 0..n {
            if r == col {
                continue;
            }
            let factor = a[r][col];
            if factor != 0.0 {
                for c in 0..2 * n {
                    a[r][c] -= factor * a[col][c];
                }
            }
        }
    }
    Some(a.into_iter().map(|row| row[n..].to_vec()).collect())
}

/// Covariance matrix of the vectors (rows = observations), with ridge.
#[allow(clippy::needless_range_loop)] // genuine 2D matrix indexing
fn covariance(vectors: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = vectors.len();
    let d = vectors.first()?.len();
    if n < 2 {
        return None;
    }
    let mean: Vec<f64> = (0..d)
        .map(|j| vectors.iter().map(|v| v[j]).sum::<f64>() / n as f64)
        .collect();
    let mut cov = vec![vec![0.0; d]; d];
    for v in vectors {
        for i in 0..d {
            for j in 0..d {
                cov[i][j] += (v[i] - mean[i]) * (v[j] - mean[j]);
            }
        }
    }
    for i in 0..d {
        for j in 0..d {
            cov[i][j] /= (n - 1) as f64;
        }
        cov[i][i] += RIDGE;
    }
    Some(cov)
}

/// Squared Mahalanobis distance between `a` and `b` given the inverse covariance.
fn mahalanobis2(a: &[f64], b: &[f64], inv_cov: &[Vec<f64>]) -> f64 {
    let d = a.len();
    let diff: Vec<f64> = (0..d).map(|i| a[i] - b[i]).collect();
    let mut acc = 0.0;
    for i in 0..d {
        let mut row = 0.0;
        for j in 0..d {
            row += inv_cov[i][j] * diff[j];
        }
        acc += diff[i] * row;
    }
    acc.max(0.0)
}

fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    Some(if lo == hi {
        sorted[lo]
    } else {
        let f = pos - lo as f64;
        sorted[lo] * (1.0 - f) + sorted[hi] * f
    })
}

/// Run the analog engine: find the k nearest historical environments to the
/// latest one, then characterize the target asset's forward returns following
/// those analog dates.
///
/// `target` is the oldest-first `(date, close)` series of the asset whose
/// forward returns we measure (may be shorter than the environment history).
/// `exclude_window` drops analogs within that many days of the query to avoid
/// trivially-recent matches.
pub fn run(
    env: &EnvironmentSeries,
    target_asset: &str,
    target: &[(String, f64)],
    horizon_days: i64,
    k: usize,
    exclude_window_days: i64,
) -> Option<AnalogReport> {
    let n = env.len();
    if n < 50 || k == 0 {
        return None;
    }
    let cov = covariance(&env.vectors)?;
    let inv = invert(&cov)?;
    let query_idx = n - 1;
    let query = &env.vectors[query_idx];
    let query_date = NaiveDate::parse_from_str(&env.dates[query_idx], "%Y-%m-%d").ok()?;

    // Distances to every prior day, excluding the recent window.
    let mut ranked: Vec<(usize, NaiveDate, f64)> = (0..query_idx)
        .filter_map(|i| {
            let d = NaiveDate::parse_from_str(&env.dates[i], "%Y-%m-%d").ok()?;
            ((query_date - d).num_days() > exclude_window_days)
                .then(|| (i, d, mahalanobis2(query, &env.vectors[i], &inv).sqrt()))
        })
        .collect();
    ranked.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());

    // De-cluster: greedily take the k closest analogs that are each at least
    // MIN_GAP_DAYS apart, so they are distinct historical EPISODES rather than
    // adjacent days from one period (the analog overlap problem).
    const MIN_GAP_DAYS: i64 = 180;
    let mut dists: Vec<(usize, f64)> = Vec::with_capacity(k);
    let mut picked_dates: Vec<NaiveDate> = Vec::new();
    for (i, d, dist) in &ranked {
        if dists.len() >= k {
            break;
        }
        if picked_dates.iter().all(|p| (*d - *p).num_days().abs() >= MIN_GAP_DAYS) {
            dists.push((*i, *dist));
            picked_dates.push(*d);
        }
    }
    if dists.is_empty() {
        return None;
    }

    // Target close lookup: BTreeMap for on/after queries.
    let tmap: BTreeMap<NaiveDate, f64> = target
        .iter()
        .filter_map(|(d, v)| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(|nd| (nd, *v)))
        .collect();
    let close_on_or_before = |d: NaiveDate| tmap.range(..=d).next_back().map(|(_, &v)| v);
    let close_on_or_after = |d: NaiveDate| tmap.range(d..).next().map(|(_, &v)| v);

    let mut analogs = Vec::with_capacity(dists.len());
    let mut fwd: Vec<f64> = Vec::new();
    for (i, dist) in &dists {
        let adate = NaiveDate::parse_from_str(&env.dates[*i], "%Y-%m-%d").ok()?;
        let fr = match (
            close_on_or_before(adate),
            close_on_or_after(adate + Duration::days(horizon_days)),
        ) {
            (Some(entry), Some(exit)) if entry > 0.0 => {
                let r = (exit / entry - 1.0) * 100.0;
                fwd.push(r);
                Some(r)
            }
            _ => None,
        };
        analogs.push(AnalogMatch {
            date: env.dates[*i].clone(),
            distance: (*dist * 1000.0).round() / 1000.0,
            regime: env.regime_quads.get(*i).cloned().unwrap_or_default(),
            forward_return_pct: fr.map(|r| (r * 100.0).round() / 100.0),
        });
    }

    let mean_distance = dists.iter().map(|(_, d)| d).sum::<f64>() / dists.len() as f64;
    let mut sorted = fwd.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n_fwd = fwd.len();
    let median_forward_pct = percentile(&sorted, 0.5);
    let mean_forward_pct = (n_fwd > 0).then(|| fwd.iter().sum::<f64>() / n_fwd as f64);
    let p25 = percentile(&sorted, 0.25);
    let p75 = percentile(&sorted, 0.75);
    let up_rate_pct = (n_fwd > 0).then(|| {
        fwd.iter().filter(|r| **r > 0.0).count() as f64 / n_fwd as f64 * 100.0
    });
    let mean_forward_ci_pct = if n_fwd >= 8 {
        let block = (n_fwd as f64).powf(1.0 / 3.0).max(2.0);
        validation::block_bootstrap_ci(
            &fwd,
            |s| Some(s.iter().sum::<f64>() / s.len() as f64),
            1000,
            block,
            0.10,
        )
        .map(|(lo, _p, hi)| ((lo * 100.0).round() / 100.0, (hi * 100.0).round() / 100.0))
    } else {
        None
    };

    let note = if n_fwd < k {
        format!(
            "only {n_fwd}/{k} analogs had {target_asset} data at the forward horizon \
             (young/short target series) — treat the distribution as indicative, not robust"
        )
    } else if n_fwd < 10 {
        "thin sample (<10 resolved analogs) — anecdotal".to_string()
    } else {
        "analog forward-return distribution over the k nearest macro environments".to_string()
    };

    Some(AnalogReport {
        query_date: env.dates[query_idx].clone(),
        query_regime: env.regime_quads.get(query_idx).cloned().unwrap_or_default(),
        target_asset: target_asset.to_string(),
        horizon_days,
        k,
        n_with_forward: n_fwd,
        analogs,
        mean_distance: (mean_distance * 1000.0).round() / 1000.0,
        median_forward_pct: median_forward_pct.map(|v| (v * 100.0).round() / 100.0),
        mean_forward_pct: mean_forward_pct.map(|v| (v * 100.0).round() / 100.0),
        p25_forward_pct: p25.map(|v| (v * 100.0).round() / 100.0),
        p75_forward_pct: p75.map(|v| (v * 100.0).round() / 100.0),
        up_rate_pct: up_rate_pct.map(|v| (v * 10.0).round() / 10.0),
        mean_forward_ci_pct,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_identity_and_roundtrip() {
        let m = vec![vec![2.0, 0.0], vec![0.0, 4.0]];
        let inv = invert(&m).unwrap();
        assert!((inv[0][0] - 0.5).abs() < 1e-9);
        assert!((inv[1][1] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn mahalanobis_zero_for_identical() {
        let inv = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        assert!(mahalanobis2(&[1.0, 2.0], &[1.0, 2.0], &inv) < 1e-12);
        assert!(mahalanobis2(&[0.0, 0.0], &[3.0, 4.0], &inv) > 0.0);
    }

    #[test]
    fn analog_finds_similar_environment_and_resolves_forward() {
        // Build a synthetic environment series where the latest vector closely
        // matches a known earlier index; verify it's picked and a forward
        // return resolves.
        let dim = 12;
        let mut dates: Vec<String> = Vec::new();
        let mut vectors: Vec<Vec<f64>> = Vec::new();
        for i in 0..300 {
            let d = NaiveDate::from_ymd_opt(2005, 1, 1).unwrap() + Duration::days(i * 3);
            dates.push(d.format("%Y-%m-%d").to_string());
            // Mostly noise, but index 50 is engineered to equal the final vector.
            let base = (i as f64 * 0.01).sin();
            vectors.push(
                (0..dim)
                    .map(|j| base + 0.001 * j as f64 + 0.01 * ((i + j) as f64).cos())
                    .collect(),
            );
        }
        // Make the last vector a near-copy of index 50.
        let copy = vectors[50].clone();
        *vectors.last_mut().unwrap() = copy;
        let env = EnvironmentSeries {
            dates: dates.clone(),
            vectors,
            feature_names: (0..dim).map(|j| format!("f{j}")).collect(),
            regime_quads: dates.iter().map(|_| "reflation".to_string()).collect(),
        };
        // Target = a rising series across the whole span.
        let target: Vec<(String, f64)> = (0..400)
            .map(|i| {
                let d = NaiveDate::from_ymd_opt(2005, 1, 1).unwrap() + Duration::days(i * 3);
                (d.format("%Y-%m-%d").to_string(), 100.0 * (1.0 + 0.002 * i as f64))
            })
            .collect();
        let rep = run(&env, "TEST", &target, 90, 10, 30).unwrap();
        assert_eq!(rep.k, 10);
        // Index 50's date should be among the nearest analogs.
        assert!(rep.analogs.iter().any(|a| a.date == dates[50]), "nearest analog should include the engineered match");
        assert!(rep.n_with_forward > 0);
        assert!(rep.median_forward_pct.is_some());
    }
}
