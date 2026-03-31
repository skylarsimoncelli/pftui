use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_cached_price_backend;
use crate::db::price_history::get_history_backend;
use crate::db::regime_snapshots;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeAssessment {
    pub regime: String,
    pub confidence: f64,
    pub drivers: Vec<String>,
    pub vix: Option<f64>,
    pub dxy: Option<f64>,
    pub yield_10y: Option<f64>,
    pub oil: Option<f64>,
    pub gold: Option<f64>,
    pub btc: Option<f64>,
}

fn latest_price(backend: &BackendConnection, symbol: &str) -> Option<f64> {
    get_cached_price_backend(backend, symbol, "USD")
        .ok()
        .flatten()
        .and_then(|q| q.price.to_string().parse::<f64>().ok())
}

fn trend_up(backend: &BackendConnection, symbol: &str, days: u32) -> Option<bool> {
    let rows = get_history_backend(backend, symbol, days + 2).ok()?;
    if rows.len() < (days as usize + 1) {
        return None;
    }
    let latest = rows.last()?.close.to_string().parse::<f64>().ok()?;
    let prev = rows[rows.len() - 1 - days as usize]
        .close
        .to_string()
        .parse::<f64>()
        .ok()?;
    Some(latest > prev)
}

pub fn classify_regime(backend: &BackendConnection) -> RegimeAssessment {
    let vix = latest_price(backend, "^VIX");
    let dxy = latest_price(backend, "DX-Y.NYB");
    let yield_10y = latest_price(backend, "^TNX");
    let oil = latest_price(backend, "CL=F");
    let gold = latest_price(backend, "GC=F");
    let btc = latest_price(backend, "BTC").or_else(|| latest_price(backend, "BTC-USD"));

    let eq_up = trend_up(backend, "SPY", 7).or_else(|| trend_up(backend, "^GSPC", 7));
    let dxy_up = trend_up(backend, "DX-Y.NYB", 7);
    let gold_up = trend_up(backend, "GC=F", 7);

    let mut drivers = Vec::new();

    let crisis_match =
        vix.map(|x| x > 30.0).unwrap_or(false) && oil.map(|x| x > 90.0).unwrap_or(false);
    if crisis_match {
        drivers.push("VIX > 30 and oil > 90".to_string());
    }

    let stagflation_match = vix.map(|x| x > 25.0).unwrap_or(false)
        && oil.map(|x| x > 80.0).unwrap_or(false)
        && gold_up.unwrap_or(false)
        && eq_up.map(|v| !v).unwrap_or(false);
    if stagflation_match {
        drivers.push("VIX > 25, oil > 80, gold up, equities down".to_string());
    }

    let risk_off_match = vix.map(|x| x > 25.0).unwrap_or(false)
        || oil.map(|x| x > 90.0).unwrap_or(false)
        || (dxy_up.unwrap_or(false)
            && gold_up.unwrap_or(false)
            && eq_up.map(|v| !v).unwrap_or(false));
    if risk_off_match {
        drivers.push("VIX/oil stress or DXY/gold up with equities down".to_string());
    }

    let risk_on_match = vix.map(|x| x < 20.0).unwrap_or(false)
        && eq_up.unwrap_or(false)
        && !dxy_up.unwrap_or(false);
    if risk_on_match {
        drivers.push("VIX < 20, equities up, DXY stable/falling".to_string());
    }

    let (regime, matched, total) = if crisis_match {
        ("crisis", 2.0, 2.0)
    } else if stagflation_match {
        ("stagflation", 4.0, 4.0)
    } else if risk_off_match {
        // Weighted confidence: volatility and energy shock should move confidence
        // more than secondary confirming signals.
        let mut matched_weight = 0.0;
        let total_weight = 1.0;
        if vix.map(|x| x > 25.0).unwrap_or(false) {
            matched_weight += 0.35;
        }
        if oil.map(|x| x > 90.0).unwrap_or(false) {
            matched_weight += 0.25;
        }
        if dxy_up.unwrap_or(false) {
            matched_weight += 0.15;
        }
        if gold_up.unwrap_or(false) {
            matched_weight += 0.10;
        }
        if eq_up.map(|v| !v).unwrap_or(false) {
            matched_weight += 0.15;
        }
        ("risk-off", matched_weight, total_weight)
    } else if risk_on_match {
        let mut m = 0.0;
        let mut t = 0.0;
        t += 1.0;
        if vix.map(|x| x < 20.0).unwrap_or(false) {
            m += 1.0;
        }
        t += 1.0;
        if eq_up.unwrap_or(false) {
            m += 1.0;
        }
        t += 1.0;
        if !dxy_up.unwrap_or(false) {
            m += 1.0;
        }
        ("risk-on", m, t)
    } else {
        ("transition", 1.0, 3.0)
    };

    RegimeAssessment {
        regime: regime.to_string(),
        confidence: if total > 0.0 { matched / total } else { 0.0 },
        drivers,
        vix,
        dxy,
        yield_10y,
        oil,
        gold,
        btc,
    }
}

pub fn classify_and_store_if_needed(backend: &BackendConnection) -> Result<bool> {
    let assessment = classify_regime(backend);
    let current = regime_snapshots::get_current_backend(backend)?;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let should_store = match current {
        None => true,
        Some(ref c) => {
            let last_date = c.recorded_at.get(0..10).unwrap_or("");
            c.regime != assessment.regime || last_date != today
        }
    };

    if should_store {
        let drivers_json = serde_json::to_string(&assessment.drivers)?;
        regime_snapshots::store_regime_backend(
            backend,
            &assessment.regime,
            Some(assessment.confidence),
            Some(&drivers_json),
            assessment.vix,
            assessment.dxy,
            assessment.yield_10y,
            assessment.oil,
            assessment.gold,
            assessment.btc,
        )?;
        return Ok(true);
    }

    Ok(false)
}

pub fn run(
    backend: &BackendConnection,
    action: &str,
    limit: Option<usize>,
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    match action {
        "current" => {
            let current = regime_snapshots::get_current_backend(backend)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "current": current }))?
                );
            } else if let Some(c) = current {
                println!(
                    "Current Regime: {} (confidence: {:.2})",
                    c.regime.to_uppercase(),
                    c.confidence.unwrap_or(0.0)
                );
                if let Some(dr) = c.drivers {
                    println!("  Drivers: {}", dr);
                }
                println!(
                    "  VIX: {:?} | DXY: {:?} | 10Y: {:?} | Oil: {:?} | Gold: {:?} | BTC: {:?}",
                    c.vix, c.dxy, c.yield_10y, c.oil, c.gold, c.btc
                );
                println!("  Recorded: {}", c.recorded_at);
            } else {
                println!("No regime snapshots yet. Run `pftui refresh`.");
            }
        }
        "history" => {
            let rows =
                regime_snapshots::get_history_filtered_backend(backend, limit, from, to)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "history": rows }))?
                );
            } else if rows.is_empty() {
                println!("No regime history.");
            } else {
                println!("Regime history ({}):", rows.len());
                for r in rows {
                    println!(
                        "  {}  {}  conf={:.2}",
                        r.recorded_at,
                        r.regime,
                        r.confidence.unwrap_or(0.0)
                    );
                }
            }
        }
        "transitions" => {
            let rows =
                regime_snapshots::get_transitions_filtered_backend(backend, limit, from, to)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "transitions": rows }))?
                );
            } else if rows.is_empty() {
                println!("No regime transitions.");
            } else {
                println!("Regime transitions ({}):", rows.len());
                for r in rows {
                    println!("  {}  {}", r.recorded_at, r.regime);
                }
            }
        }
        "summary" => {
            run_summary(backend, from, to, json_output)?;
        }
        other => anyhow::bail!(
            "unknown regime action '{}'. Valid: current, history, transitions, summary",
            other
        ),
    }

    Ok(())
}

/// Summary statistics for regime history: time in each regime, transition counts, durations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSummary {
    /// Total snapshots in the queried range.
    pub total_snapshots: usize,
    /// Total regime transitions detected.
    pub total_transitions: usize,
    /// Date range covered.
    pub date_range: Option<DateRange>,
    /// Per-regime breakdown: snapshots, percentage of time, avg confidence.
    pub regimes: Vec<RegimeStats>,
    /// Transition pairs with counts and examples.
    pub transition_pairs: Vec<TransitionPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    pub from: String,
    pub to: String,
    pub total_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeStats {
    pub regime: String,
    pub snapshot_count: usize,
    pub percentage: f64,
    pub avg_confidence: f64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionPair {
    pub from: String,
    pub to: String,
    pub count: usize,
    pub last_occurred: String,
}

fn run_summary(
    backend: &BackendConnection,
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let rows = regime_snapshots::get_history_filtered_backend(backend, None, from, to)?;

    if rows.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "total_snapshots": 0,
                    "total_transitions": 0,
                    "regimes": [],
                    "transition_pairs": []
                }))?
            );
        } else {
            println!("No regime data in the specified range.");
        }
        return Ok(());
    }

    // Rows are desc — reverse for chronological processing
    let chronological: Vec<_> = rows.iter().rev().collect();

    // Date range
    let first_date = chronological
        .first()
        .map(|r| r.recorded_at.get(0..10).unwrap_or(&r.recorded_at).to_string())
        .unwrap_or_default();
    let last_date = chronological
        .last()
        .map(|r| r.recorded_at.get(0..10).unwrap_or(&r.recorded_at).to_string())
        .unwrap_or_default();
    let total_days = {
        let d1 = chrono::NaiveDate::parse_from_str(&first_date, "%Y-%m-%d");
        let d2 = chrono::NaiveDate::parse_from_str(&last_date, "%Y-%m-%d");
        match (d1, d2) {
            (Ok(a), Ok(b)) => (b - a).num_days().max(1),
            _ => 1,
        }
    };

    let date_range = Some(DateRange {
        from: first_date,
        to: last_date,
        total_days,
    });

    // Per-regime stats
    let mut regime_map: BTreeMap<String, (usize, f64, String, String)> = BTreeMap::new();
    for r in &chronological {
        let entry = regime_map
            .entry(r.regime.clone())
            .or_insert((0, 0.0, r.recorded_at.clone(), r.recorded_at.clone()));
        entry.0 += 1;
        entry.1 += r.confidence.unwrap_or(0.0);
        // Update first/last seen
        if r.recorded_at < entry.2 {
            entry.2 = r.recorded_at.clone();
        }
        if r.recorded_at > entry.3 {
            entry.3 = r.recorded_at.clone();
        }
    }

    let total = chronological.len();
    let mut regimes: Vec<RegimeStats> = regime_map
        .into_iter()
        .map(|(regime, (count, conf_sum, first, last))| RegimeStats {
            regime,
            snapshot_count: count,
            percentage: (count as f64 / total as f64 * 100.0 * 10.0).round() / 10.0,
            avg_confidence: if count > 0 {
                (conf_sum / count as f64 * 100.0).round() / 100.0
            } else {
                0.0
            },
            first_seen: first.get(0..19).unwrap_or(&first).to_string(),
            last_seen: last.get(0..19).unwrap_or(&last).to_string(),
        })
        .collect();
    regimes.sort_by(|a, b| b.snapshot_count.cmp(&a.snapshot_count));

    // Transition pairs
    let mut pair_map: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    let mut transition_count = 0;
    for i in 1..chronological.len() {
        if chronological[i].regime != chronological[i - 1].regime {
            transition_count += 1;
            let key = (
                chronological[i - 1].regime.clone(),
                chronological[i].regime.clone(),
            );
            let entry = pair_map
                .entry(key)
                .or_insert((0, chronological[i].recorded_at.clone()));
            entry.0 += 1;
            if chronological[i].recorded_at > entry.1 {
                entry.1 = chronological[i].recorded_at.clone();
            }
        }
    }

    let mut transition_pairs: Vec<TransitionPair> = pair_map
        .into_iter()
        .map(|((from, to), (count, last))| TransitionPair {
            from,
            to,
            count,
            last_occurred: last.get(0..19).unwrap_or(&last).to_string(),
        })
        .collect();
    transition_pairs.sort_by(|a, b| b.count.cmp(&a.count));

    let summary = RegimeSummary {
        total_snapshots: total,
        total_transitions: transition_count,
        date_range,
        regimes,
        transition_pairs,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("═══ Regime Summary ═══");
        if let Some(ref dr) = summary.date_range {
            println!("Period: {} to {} ({} days)", dr.from, dr.to, dr.total_days);
        }
        println!(
            "Snapshots: {} | Transitions: {}",
            summary.total_snapshots, summary.total_transitions
        );
        println!();

        println!("── Time in Regime ──");
        for r in &summary.regimes {
            println!(
                "  {:14} {:>4} snapshots ({:>5.1}%)  avg conf: {:.2}",
                r.regime.to_uppercase(),
                r.snapshot_count,
                r.percentage,
                r.avg_confidence
            );
        }
        println!();

        if !summary.transition_pairs.is_empty() {
            println!("── Transition Pairs ──");
            for t in &summary.transition_pairs {
                println!(
                    "  {} → {}  (×{}, last: {})",
                    t.from, t.to, t.count, t.last_occurred
                );
            }
        }
    }

    Ok(())
}

pub fn run_set(
    backend: &BackendConnection,
    regime: &str,
    confidence: Option<f64>,
    drivers: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let regime = regime.trim().to_lowercase();
    if regime.is_empty() {
        anyhow::bail!("regime name required");
    }

    let assessment = classify_regime(backend);
    let confidence = confidence.or_else(|| {
        if assessment.regime == regime {
            Some(assessment.confidence)
        } else {
            None
        }
    });
    let drivers_json = if let Some(reason) = drivers {
        Some(serde_json::to_string(&vec![reason])?)
    } else if assessment.regime == regime && !assessment.drivers.is_empty() {
        Some(serde_json::to_string(&assessment.drivers)?)
    } else {
        None
    };

    regime_snapshots::store_regime_backend(
        backend,
        &regime,
        confidence,
        drivers_json.as_deref(),
        assessment.vix,
        assessment.dxy,
        assessment.yield_10y,
        assessment.oil,
        assessment.gold,
        assessment.btc,
    )?;

    let current = regime_snapshots::get_current_backend(backend)?
        .ok_or_else(|| anyhow::anyhow!("failed to load stored regime snapshot"))?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&current)?);
    } else {
        println!(
            "Set macro regime to {} ({})",
            current.regime,
            current
                .confidence
                .map(|value| format!("confidence: {:.2}", value))
                .unwrap_or_else(|| "confidence: n/a".to_string())
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    /// Helper to insert a regime snapshot at a specific timestamp.
    fn insert_snapshot(
        backend: &BackendConnection,
        regime: &str,
        confidence: f64,
        timestamp: &str,
    ) {
        let BackendConnection::Sqlite { conn } = backend else {
            panic!("expected sqlite");
        };
        conn.execute(
            "INSERT INTO regime_snapshots (regime, confidence, drivers, recorded_at)
             VALUES (?, ?, NULL, ?)",
            rusqlite::params![regime, confidence, timestamp],
        )
        .unwrap();
    }

    #[test]
    fn run_set_stores_manual_regime_snapshot() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        run_set(
            &backend,
            "risk-off",
            Some(0.8),
            Some("manual override"),
            true,
        )
        .unwrap();

        let current = regime_snapshots::get_current_backend(&backend)
            .unwrap()
            .unwrap();
        assert_eq!(current.regime, "risk-off");
        assert_eq!(current.confidence, Some(0.8));
        assert_eq!(current.drivers.as_deref(), Some("[\"manual override\"]"));
    }

    #[test]
    fn history_filtered_by_from_date() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-25 10:00:00");
        insert_snapshot(&backend, "crisis", 1.0, "2026-03-30 10:00:00");

        let rows = regime_snapshots::get_history_filtered_backend(
            &backend,
            None,
            Some("2026-03-25"),
            None,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        // Desc order: crisis first, then risk-off
        assert_eq!(rows[0].regime, "crisis");
        assert_eq!(rows[1].regime, "risk-off");
    }

    #[test]
    fn history_filtered_by_to_date() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-25 10:00:00");
        insert_snapshot(&backend, "crisis", 1.0, "2026-03-30 10:00:00");

        let rows = regime_snapshots::get_history_filtered_backend(
            &backend,
            None,
            None,
            Some("2026-03-25"),
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].regime, "risk-off");
        assert_eq!(rows[1].regime, "risk-on");
    }

    #[test]
    fn history_filtered_by_date_range() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-25 10:00:00");
        insert_snapshot(&backend, "crisis", 1.0, "2026-03-30 10:00:00");

        let rows = regime_snapshots::get_history_filtered_backend(
            &backend,
            None,
            Some("2026-03-24"),
            Some("2026-03-26"),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].regime, "risk-off");
    }

    #[test]
    fn transitions_filtered_by_date_range() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-22 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-25 10:00:00");
        insert_snapshot(&backend, "crisis", 1.0, "2026-03-30 10:00:00");

        // All transitions
        let all = regime_snapshots::get_transitions_filtered_backend(
            &backend,
            None,
            None,
            None,
        )
        .unwrap();
        // Should be 3: crisis, risk-off, risk-on (deduplicated, desc order)
        assert_eq!(all.len(), 3);

        // Only from 2026-03-24 onwards: crisis and risk-off
        let filtered = regime_snapshots::get_transitions_filtered_backend(
            &backend,
            None,
            Some("2026-03-24"),
            None,
        )
        .unwrap();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].regime, "crisis");
        assert_eq!(filtered[1].regime, "risk-off");
    }

    #[test]
    fn summary_with_data() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-on", 0.85, "2026-03-21 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-25 10:00:00");
        insert_snapshot(&backend, "crisis", 1.0, "2026-03-28 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.6, "2026-03-30 10:00:00");

        // Run summary in JSON mode to verify output
        run_summary(&backend, None, None, true).unwrap();
    }

    #[test]
    fn summary_empty_data() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        run_summary(&backend, None, None, true).unwrap();
    }

    #[test]
    fn summary_date_filtered() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-25 10:00:00");
        insert_snapshot(&backend, "crisis", 1.0, "2026-03-30 10:00:00");

        // Only from Mar 24 onwards
        run_summary(&backend, Some("2026-03-24"), None, true).unwrap();
    }

    #[test]
    fn summary_regime_stats_correct() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-on", 0.9, "2026-03-21 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.6, "2026-03-25 10:00:00");

        let rows =
            regime_snapshots::get_history_filtered_backend(&backend, None, None, None).unwrap();
        let chronological: Vec<_> = rows.iter().rev().collect();
        assert_eq!(chronological.len(), 3);

        // Verify regime counts
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &chronological {
            *counts.entry(&r.regime).or_default() += 1;
        }
        assert_eq!(counts["risk-on"], 2);
        assert_eq!(counts["risk-off"], 1);
    }

    #[test]
    fn summary_transition_pairs_counted() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-20 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-22 10:00:00");
        insert_snapshot(&backend, "risk-on", 0.8, "2026-03-24 10:00:00");
        insert_snapshot(&backend, "risk-off", 0.7, "2026-03-26 10:00:00");

        let rows =
            regime_snapshots::get_history_filtered_backend(&backend, None, None, None).unwrap();
        let chronological: Vec<_> = rows.iter().rev().collect();

        // Count transitions
        let mut transitions = 0;
        for i in 1..chronological.len() {
            if chronological[i].regime != chronological[i - 1].regime {
                transitions += 1;
            }
        }
        assert_eq!(transitions, 3); // on→off, off→on, on→off
    }
}
