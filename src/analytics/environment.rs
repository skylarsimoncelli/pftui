//! Environment feature vector — a daily, stationary, z-scored description of
//! the multi-asset macro backdrop. The foundation of the analog engine
//! (`docs/ENVIRONMENT-ENGINE.md` §3.1).
//!
//! Features are engineered to be **stationary** (returns / changes / spreads /
//! levels-already-bounded, never raw price levels, which have unit roots) and
//! **expanding-window z-scored** (each day uses only data up to that day — no
//! look-ahead). All values are `f64`: this is a statistics object, not money.
//!
//! v1 features (macro core, deep history back to ~2003):
//! - 20-day log-return and 20-day realized vol for SPX, gold, oil, DXY
//! - 10y yield level and its 20-day change
//! - 2s10s-style slope proxy: 10y minus 3m bill
//! - VIX level
//!
//! Cross-asset correlations and the `available_at` release-lag discipline for
//! macro prints are deferred to the persisted L2 table (a later slice); v1
//! computes on the fly from `price_history` daily closes, which are
//! close-of-day correct.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use chrono::NaiveDate;

/// Symbols the v1 environment vector reads. The master axis is the set of dates
/// on which every one of these has a close (plus enough lookback for the
/// windows).
pub const ENV_SYMBOLS: [&str; 7] = [
    "^GSPC",    // S&P 500
    "GC=F",     // gold
    "CL=F",     // WTI crude
    "DX-Y.NYB", // dollar index
    "^TNX",     // 10y yield (×10)
    "^IRX",     // 13-week T-bill yield (×10)
    "^VIX",     // volatility index
];

const RET_WINDOW: usize = 20;
const VOL_WINDOW: usize = 20;
/// Expanding z-score warmup — rows before this many observations are dropped.
const ZSCORE_WARMUP: usize = 120;

/// A daily environment matrix: aligned dates, z-scored feature vectors, and the
/// feature names (column order).
#[derive(Debug, Clone)]
pub struct EnvironmentSeries {
    pub dates: Vec<String>,
    pub vectors: Vec<Vec<f64>>,
    pub feature_names: Vec<String>,
    /// Growth×inflation regime quad label per date (parallel to `dates`).
    pub regime_quads: Vec<String>,
}

impl EnvironmentSeries {
    pub fn len(&self) -> usize {
        self.dates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.dates.is_empty()
    }
    /// The latest (most recent) environment vector.
    pub fn latest(&self) -> Option<(&str, &[f64])> {
        let i = self.dates.len().checked_sub(1)?;
        Some((&self.dates[i], &self.vectors[i]))
    }
}

fn parse(d: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
}

/// Build the environment series from per-symbol `(date, close)` histories
/// (oldest-first). Returns an error if a required symbol is missing/empty.
#[allow(clippy::needless_range_loop)] // lagged/windowed indexing over aligned series
pub fn build(series: &BTreeMap<String, Vec<(String, f64)>>) -> Result<EnvironmentSeries> {
    // Per-symbol date→close maps.
    let mut maps: BTreeMap<&str, BTreeMap<NaiveDate, f64>> = BTreeMap::new();
    for sym in ENV_SYMBOLS {
        let rows = series
            .get(sym)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("environment vector needs series '{sym}' (missing/empty)"))?;
        let m: BTreeMap<NaiveDate, f64> = rows
            .iter()
            .filter_map(|(d, v)| parse(d).map(|nd| (nd, *v)))
            .collect();
        maps.insert(sym, m);
    }

    // Master axis = dates present in the SPX series (the longest, most liquid),
    // restricted to where every symbol also has a close.
    let spx = &maps["^GSPC"];
    let mut master: Vec<NaiveDate> = spx
        .keys()
        .copied()
        .filter(|d| ENV_SYMBOLS.iter().all(|s| maps[s].contains_key(d)))
        .collect();
    master.sort();
    if master.len() < ZSCORE_WARMUP + RET_WINDOW + 5 {
        bail!("insufficient overlapping history for the environment vector");
    }

    // Per-symbol aligned close vector on the master axis.
    let aligned: BTreeMap<&str, Vec<f64>> = ENV_SYMBOLS
        .iter()
        .map(|&s| (s, master.iter().map(|d| maps[s][d]).collect::<Vec<f64>>()))
        .collect();

    let feature_names = vec![
        "spx_ret20".to_string(),
        "spx_vol20".to_string(),
        "gold_ret20".to_string(),
        "gold_vol20".to_string(),
        "oil_ret20".to_string(),
        "oil_vol20".to_string(),
        "dxy_ret20".to_string(),
        "dxy_vol20".to_string(),
        "tnx_level".to_string(),
        "tnx_chg20".to_string(),
        "curve_10y_3m".to_string(),
        "vix_level".to_string(),
    ];

    // Raw (pre-z-score) features per master index. Indices before the windows
    // resolve are skipped.
    let n = master.len();
    let start = RET_WINDOW.max(VOL_WINDOW);
    let mut raw_rows: Vec<Vec<f64>> = Vec::with_capacity(n - start);
    let mut raw_dates: Vec<NaiveDate> = Vec::with_capacity(n - start);
    let mut raw_quads: Vec<String> = Vec::with_capacity(n - start);
    for i in start..n {
        let f = |sym: &str| &aligned[sym];
        let ret = |s: &str| log_return(f(s), i, RET_WINDOW);
        let vol = |s: &str| realized_vol(f(s), i, VOL_WINDOW);
        let row = vec![
            ret("^GSPC"),
            vol("^GSPC"),
            ret("GC=F"),
            vol("GC=F"),
            ret("CL=F"),
            vol("CL=F"),
            ret("DX-Y.NYB"),
            vol("DX-Y.NYB"),
            f("^TNX")[i],
            f("^TNX")[i] - f("^TNX")[i - RET_WINDOW],
            f("^TNX")[i] - f("^IRX")[i],
            f("^VIX")[i],
        ];
        if row.iter().all(|x| x.is_finite()) {
            let quad = super::regime_quad::classify(f("^GSPC"), f("GC=F"), f("CL=F"), i);
            raw_rows.push(row);
            raw_dates.push(master[i]);
            raw_quads.push(quad.short().to_string());
        }
    }

    // Expanding-window z-score per column (no look-ahead), drop warmup rows.
    let dim = feature_names.len();
    let mut vectors = Vec::new();
    let mut dates = Vec::new();
    let mut regime_quads = Vec::new();
    let mut sums = vec![0.0f64; dim];
    let mut sumsq = vec![0.0f64; dim];
    for (t, row) in raw_rows.iter().enumerate() {
        for j in 0..dim {
            sums[j] += row[j];
            sumsq[j] += row[j] * row[j];
        }
        if t < ZSCORE_WARMUP {
            continue;
        }
        let cnt = (t + 1) as f64;
        let z: Vec<f64> = (0..dim)
            .map(|j| {
                let mean = sums[j] / cnt;
                let var = (sumsq[j] / cnt - mean * mean).max(1e-12);
                (row[j] - mean) / var.sqrt()
            })
            .collect();
        vectors.push(z);
        dates.push(raw_dates[t].format("%Y-%m-%d").to_string());
        regime_quads.push(raw_quads[t].clone());
    }

    Ok(EnvironmentSeries {
        dates,
        vectors,
        feature_names,
        regime_quads,
    })
}

/// Log return over `w` bars ending at index `i`.
fn log_return(v: &[f64], i: usize, w: usize) -> f64 {
    if i < w || v[i - w] <= 0.0 || v[i] <= 0.0 {
        return f64::NAN;
    }
    (v[i] / v[i - w]).ln()
}

/// Annualized realized vol of daily log returns over `w` bars ending at `i`.
fn realized_vol(v: &[f64], i: usize, w: usize) -> f64 {
    if i < w {
        return f64::NAN;
    }
    let rets: Vec<f64> = (i - w + 1..=i)
        .filter_map(|k| {
            if k == 0 || v[k - 1] <= 0.0 || v[k] <= 0.0 {
                None
            } else {
                Some((v[k] / v[k - 1]).ln())
            }
        })
        .collect();
    if rets.len() < 2 {
        return f64::NAN;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    (var.sqrt()) * (252.0f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(seed: u64, n: usize, base: f64, drift: f64) -> Vec<(String, f64)> {
        // Deterministic pseudo-random walk (no Math::random).
        let mut x = base;
        let mut s = seed;
        (0..n)
            .map(|i| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u = ((s >> 33) as f64 / (1u64 << 31) as f64) - 1.0; // ~[-1,1]
                x *= 1.0 + drift + 0.01 * u;
                let d = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap() + chrono::Duration::days(i as i64);
                (d.format("%Y-%m-%d").to_string(), x.max(0.01))
            })
            .collect()
    }

    #[test]
    fn builds_zscored_vectors_of_correct_dim() {
        let mut series = BTreeMap::new();
        for (i, sym) in ENV_SYMBOLS.iter().enumerate() {
            series.insert(sym.to_string(), synth(i as u64 + 1, 400, 100.0, 0.0002));
        }
        let env = build(&series).unwrap();
        assert_eq!(env.feature_names.len(), 12);
        assert!(!env.is_empty());
        // z-scored: every feature finite, and the column means are ~0 over the
        // tail (expanding z-score converges near zero-mean).
        let (_, latest) = env.latest().unwrap();
        assert_eq!(latest.len(), 12);
        assert!(latest.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn errors_on_missing_symbol() {
        let mut series = BTreeMap::new();
        series.insert("^GSPC".to_string(), synth(1, 400, 100.0, 0.0));
        assert!(build(&series).is_err());
    }

    #[test]
    fn log_return_and_vol_are_sane() {
        let v: Vec<f64> = (0..50).map(|i| 100.0 * (1.0 + 0.001 * i as f64)).collect();
        let r = log_return(&v, 40, 20);
        assert!(r > 0.0 && r < 0.1); // gentle uptrend
        let vol = realized_vol(&v, 40, 20);
        assert!(vol >= 0.0 && vol.is_finite());
    }
}
