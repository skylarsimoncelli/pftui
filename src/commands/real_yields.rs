//! CLI commands for the real-yields curve: refresh, show, and US-vs-G10 differentials.
//!
//! Network calls run through `tokio::runtime::Runtime::block_on` so the entire
//! pipeline stays usable from synchronous CLI dispatch. When the FRED API key
//! is missing or the network is unreachable, the refresh routine degrades to a
//! no-op and reports zero observations rather than failing the command.

use anyhow::Result;
use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::config::Config;
use crate::data::real_yields::{
    all_series_ids, compute_differentials, fetch_series_history, DifferentialSnapshot,
    PairDifferential, RealYieldObservation,
};
use crate::db::backend::BackendConnection;
use crate::db::real_yields_history::{
    fetch_history_backend, fetch_latest_per_series_backend, upsert_observations_backend, RealYieldRow,
};

#[derive(Debug, Clone, Serialize)]
struct RefreshSummary {
    series_attempted: usize,
    series_with_data: usize,
    observations_written: usize,
    fred_api_key_present: bool,
    days_requested: u32,
    notes: Vec<String>,
}

/// Fetch all configured real-yield series from FRED and persist them.
///
/// Idempotent and offline-safe: missing API key, network errors, or empty
/// observations all collapse to a clean summary instead of an error.
pub fn refresh(backend: &BackendConnection, config: &Config, days: u32, json: bool) -> Result<()> {
    let api_key = config
        .fred_api_key
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let key_present = !api_key.is_empty();
    let mut notes: Vec<String> = Vec::new();
    if !key_present {
        notes.push("FRED API key absent — skipping live fetch (degraded mode).".to_string());
    }

    let rt = tokio::runtime::Runtime::new()?;
    let series_ids = all_series_ids();
    let mut all_obs: Vec<RealYieldObservation> = Vec::new();
    let mut series_with_data = 0usize;

    if key_present {
        for sid in &series_ids {
            let result = rt.block_on(fetch_series_history(&api_key, sid, days));
            match result {
                Ok(obs) if !obs.is_empty() => {
                    series_with_data += 1;
                    all_obs.extend(obs);
                }
                Ok(_) => {
                    notes.push(format!("{}: no observations returned", sid));
                }
                Err(e) => {
                    notes.push(format!("{}: fetch error: {}", sid, e));
                }
            }
        }
    }

    upsert_observations_backend(backend, &all_obs)?;

    let summary = RefreshSummary {
        series_attempted: series_ids.len(),
        series_with_data,
        observations_written: all_obs.len(),
        fred_api_key_present: key_present,
        days_requested: days,
        notes,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "real-yields refresh: {}/{} series with data, {} rows written ({} days)",
            summary.series_with_data,
            summary.series_attempted,
            summary.observations_written,
            summary.days_requested
        );
        if !summary.fred_api_key_present {
            println!("  fred_api_key not configured — ran in degraded (offline) mode");
        }
        for note in &summary.notes {
            println!("  note: {}", note);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct ShowResponse<'a> {
    series_filter: Option<&'a str>,
    since: Option<String>,
    row_count: usize,
    rows: Vec<RealYieldRow>,
}

/// `pftui data real-yields show` — read cached real-yield rows, optionally
/// filtered by series id and a relative `since` window such as `30d`.
pub fn show(
    backend: &BackendConnection,
    series: Option<&str>,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    let since_date = since.map(parse_since_to_date).transpose()?;
    let rows = fetch_history_backend(backend, series, since_date.as_deref())?;

    if json {
        let resp = ShowResponse {
            series_filter: series,
            since: since_date.clone(),
            row_count: rows.len(),
            rows: rows.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        let label = series.unwrap_or("(all series)");
        let window = since_date.as_deref().unwrap_or("(all dates)");
        println!(
            "real-yields show: series={} window>={} rows={}",
            label,
            window,
            rows.len()
        );
        for r in rows.iter().take(50) {
            println!("  {}  {:<20}  {:>8.3}  [{}]", r.date, r.series, r.value, r.source);
        }
        if rows.len() > 50 {
            println!("  ... {} more rows (use --json for full data)", rows.len() - 50);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct DifferentialsResponse {
    since: Option<String>,
    snapshot_count: usize,
    snapshots: Vec<DifferentialSnapshotJson>,
}

#[derive(Debug, Clone, Serialize)]
struct DifferentialSnapshotJson {
    date: String,
    us_tips_10y: Option<f64>,
    us_breakeven_10y: Option<f64>,
    us_nominal_10y: Option<f64>,
    us_minus_g10_avg_bp: Option<f64>,
    pairs: Vec<PairDifferentialJson>,
}

#[derive(Debug, Clone, Serialize)]
struct PairDifferentialJson {
    country: String,
    partner_series: String,
    us_value: f64,
    partner_value: f64,
    spread_bp: f64,
}

impl From<DifferentialSnapshot> for DifferentialSnapshotJson {
    fn from(s: DifferentialSnapshot) -> Self {
        Self {
            date: s.date,
            us_tips_10y: s.us_tips_10y,
            us_breakeven_10y: s.us_breakeven_10y,
            us_nominal_10y: s.us_nominal_10y,
            us_minus_g10_avg_bp: s.us_minus_g10_avg_bp,
            pairs: s
                .pairs
                .into_iter()
                .map(|p: PairDifferential| PairDifferentialJson {
                    country: p.country,
                    partner_series: p.partner_series,
                    us_value: p.us_value,
                    partner_value: p.partner_value,
                    spread_bp: p.spread_bp,
                })
                .collect(),
        }
    }
}

/// `pftui analytics real-rates differentials` — compute US-vs-G10 spreads from
/// the cached series and emit them as a per-day snapshot list.
pub fn differentials(backend: &BackendConnection, since: Option<&str>, json: bool) -> Result<()> {
    let since_date = since.map(parse_since_to_date).transpose()?;
    // The G10 OECD series are MONTHLY and can lag a month or two, so load ~90
    // days BEFORE the display window to seed the forward-fill carry — otherwise
    // a narrow recent window contains no monthly G10 print and every pair is
    // empty. Snapshots are filtered back to the requested window after.
    let load_from = since_date.as_deref().and_then(|d| {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .ok()
            .map(|nd| (nd - chrono::Duration::days(90)).format("%Y-%m-%d").to_string())
    });
    let rows = fetch_history_backend(backend, None, load_from.as_deref().or(since_date.as_deref()))?;
    let tuples = rows
        .into_iter()
        .map(|r| (r.date, r.series, r.value))
        .collect::<Vec<_>>();
    let snapshots: Vec<_> = compute_differentials(tuples)
        .into_iter()
        .filter(|s| since_date.as_deref().is_none_or(|d| s.date.as_str() >= d))
        .collect();

    if json {
        let resp = DifferentialsResponse {
            since: since_date.clone(),
            snapshot_count: snapshots.len(),
            snapshots: snapshots.into_iter().map(Into::into).collect(),
        };
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        println!(
            "real-rates differentials: {} day-snapshots (window>={})",
            snapshots.len(),
            since_date.as_deref().unwrap_or("(all)")
        );
        println!(
            "  (us-vs-g10 spread compares NOMINAL US 10y vs NOMINAL G10 OECD 10y; the tips10y/be10y columns are the US REAL/breakeven legs. G10 prints are monthly, forward-filled to each US day.)"
        );
        for snap in snapshots.iter().take(20) {
            let avg = snap
                .us_minus_g10_avg_bp
                .map(|v| format!("{:+.1}bp", v))
                .unwrap_or_else(|| "n/a".to_string());
            println!(
                "  {}  us10y={}  tips10y={}  be10y={}  us-vs-g10-avg={}  pairs={}",
                snap.date,
                opt_fmt(snap.us_nominal_10y),
                opt_fmt(snap.us_tips_10y),
                opt_fmt(snap.us_breakeven_10y),
                avg,
                snap.pairs.len()
            );
        }
    }
    Ok(())
}

fn opt_fmt(v: Option<f64>) -> String {
    v.map(|x| format!("{:.3}", x)).unwrap_or_else(|| "n/a".into())
}

/// Parse a `since` argument as either an absolute YYYY-MM-DD date or a
/// relative `Nd`/`Nw`/`Nm` window (months treated as 30 days).
pub(crate) fn parse_since_to_date(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(d.format("%Y-%m-%d").to_string());
    }
    let (num_part, unit) = trimmed.split_at(
        trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len()),
    );
    let n: i64 = num_part
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --since: expected NNd/NNw/NNm or YYYY-MM-DD, got '{}'", input))?;
    let days = match unit {
        "d" | "" => n,
        "w" => n * 7,
        "m" => n * 30,
        other => anyhow::bail!("unknown --since unit '{}' (use d/w/m or YYYY-MM-DD)", other),
    };
    let date = Utc::now().date_naive() - Duration::days(days);
    Ok(date.format("%Y-%m-%d").to_string())
}

/// Inline summary used by the report Macro section. Returns `None` if there is
/// nothing in cache to render — keeps callers from emitting empty blocks.
#[allow(dead_code)] // Consumed by report::sections::real_rates_macro and analyst routines (P3 hook)
pub fn latest_macro_snapshot(backend: &BackendConnection) -> Result<Option<MacroBlockSnapshot>> {
    let latest = fetch_latest_per_series_backend(backend)?;
    if latest.is_empty() {
        return Ok(None);
    }
    let find = |s: &str| latest.iter().find(|r| r.series == s).map(|r| r.value);
    let us_10y = find("DGS10");
    let tips_10y = find("DFII10");
    let be_10y = find("T10YIE");
    let de_10y = find("IRLTLT01DEM156N");

    // Week-ago value for 10Y TIPS — use the oldest row inside a 10-day lookback
    // (calendar days; FRED-business-day gaps are tolerated).
    let since = (Utc::now().date_naive() - Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let tips_window = fetch_history_backend(backend, Some("DFII10"), Some(&since)).unwrap_or_default();
    let tips_week_change_bp = if tips_window.len() >= 2 {
        let oldest = tips_window.first().map(|r| r.value);
        let newest = tips_window.last().map(|r| r.value);
        match (oldest, newest) {
            (Some(o), Some(n)) => Some((n - o) * 100.0),
            _ => None,
        }
    } else {
        None
    };

    Ok(Some(MacroBlockSnapshot {
        us_nominal_10y: us_10y,
        us_tips_10y: tips_10y,
        us_breakeven_10y: be_10y,
        us_minus_de_bp: us_10y.zip(de_10y).map(|(a, b)| (a - b) * 100.0),
        tips_week_change_bp,
    }))
}

/// Compact snapshot used by `real_rates_macro::render_real_rates_block`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroBlockSnapshot {
    pub us_nominal_10y: Option<f64>,
    pub us_tips_10y: Option<f64>,
    pub us_breakeven_10y: Option<f64>,
    pub us_minus_de_bp: Option<f64>,
    pub tips_week_change_bp: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::real_yields::RealYieldObservation;
    use crate::db::real_yields_history::upsert_observations;
    use crate::db::schema;
    use rusqlite::Connection;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        schema::run_migrations(&c).unwrap();
        c
    }

    #[test]
    fn parse_since_handles_days_weeks_months_and_iso() {
        assert_eq!(parse_since_to_date("2026-04-01").unwrap(), "2026-04-01");
        // Relative inputs are anchored to today; just check the format is right.
        let d = parse_since_to_date("30d").unwrap();
        assert_eq!(d.len(), 10);
        let w = parse_since_to_date("2w").unwrap();
        assert_eq!(w.len(), 10);
        let m = parse_since_to_date("3m").unwrap();
        assert_eq!(m.len(), 10);
        assert!(parse_since_to_date("bogus").is_err());
    }

    #[test]
    fn latest_macro_snapshot_returns_none_for_empty_db() {
        let c = mem();
        let backend = BackendConnection::Sqlite { conn: c };
        let snap = latest_macro_snapshot(&backend).expect("ok");
        assert!(snap.is_none());
    }

    #[test]
    fn latest_macro_snapshot_assembles_from_fixture_rows() {
        let c = mem();
        let today = Utc::now().date_naive();
        let week_ago = (today - Duration::days(6)).format("%Y-%m-%d").to_string();
        let today_str = today.format("%Y-%m-%d").to_string();
        upsert_observations(
            &c,
            &[
                RealYieldObservation {
                    series_id: "DGS10".into(),
                    date: today_str.clone(),
                    value: 4.30,
                    source: "FRED".into(),
                },
                RealYieldObservation {
                    series_id: "DFII10".into(),
                    date: week_ago.clone(),
                    value: 2.00,
                    source: "FRED".into(),
                },
                RealYieldObservation {
                    series_id: "DFII10".into(),
                    date: today_str.clone(),
                    value: 2.15,
                    source: "FRED".into(),
                },
                RealYieldObservation {
                    series_id: "T10YIE".into(),
                    date: today_str.clone(),
                    value: 2.40,
                    source: "FRED".into(),
                },
                RealYieldObservation {
                    series_id: "IRLTLT01DEM156N".into(),
                    date: today_str,
                    value: 2.20,
                    source: "FRED".into(),
                },
            ],
        )
        .expect("upsert");
        let backend = BackendConnection::Sqlite { conn: c };
        let snap = latest_macro_snapshot(&backend)
            .expect("ok")
            .expect("snapshot present");
        assert_eq!(snap.us_nominal_10y, Some(4.30));
        assert_eq!(snap.us_tips_10y, Some(2.15));
        assert_eq!(snap.us_breakeven_10y, Some(2.40));
        // 4.30 - 2.20 = 2.10 → 210 bp
        let diff = snap.us_minus_de_bp.expect("diff present");
        assert!((diff - 210.0).abs() < 1e-6);
        // TIPS week change: 2.15 - 2.00 = 0.15 → +15 bp
        let wk = snap.tips_week_change_bp.expect("week change present");
        assert!((wk - 15.0).abs() < 1e-6);
    }
}
