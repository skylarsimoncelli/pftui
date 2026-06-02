//! Scenario-conditional backtest — filter scored predictions by the regime
//! they were made under and compute regime-aware hit rates.
//!
//! Two CLI shapes are exposed:
//!
//! 1. `analytics backtest scenario` — given a probability range per scenario
//!    (or a `--regime` preset), return the hit rate of the matching cohort.
//! 2. `analytics backtest layer-bias` — same shape as the calibration matrix
//!    but conditioned on the regime (LOW / MEDIUM / HIGH / MACRO × topic).
//!
//! The join is:
//!
//! ```text
//! scenario_prediction_links spl
//!   JOIN user_predictions up ON up.id = spl.prediction_id
//!   JOIN scenarios s ON s.id = spl.scenario_id
//! ```
//!
//! The schema mirrors how scenarios are surfaced today: `scenario_prediction_links`
//! holds the per-scenario probability at write time (see
//! `src/db/schema.rs` for the table definition). For this backtest we
//! conservatively treat the link table as a *snapshot of probabilities at
//! prediction time*, falling back to the scenario's latest probability when
//! no explicit snapshot column exists.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::regime_history::{self, RegimeFilter};

/// One outcome bucket from a scenario-conditional backtest.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScenarioBacktestSummary {
    pub regime: Option<String>,
    pub filter: SerializedFilter,
    pub layer: Option<String>,
    pub topic: Option<String>,
    pub conviction: Option<String>,
    pub matched_predictions: usize,
    pub scored_predictions: usize,
    pub correct: usize,
    pub partial: usize,
    pub wrong: usize,
    pub pending: usize,
    pub hit_rate_pct: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SerializedFilter {
    pub inflation_min: Option<f64>,
    pub inflation_max: Option<f64>,
    pub recession_min: Option<f64>,
    pub recession_max: Option<f64>,
    pub iran_min: Option<f64>,
    pub iran_max: Option<f64>,
    pub risk_on_min: Option<f64>,
    pub risk_on_max: Option<f64>,
}

impl From<RegimeFilter> for SerializedFilter {
    fn from(f: RegimeFilter) -> Self {
        Self {
            inflation_min: f.inflation_min,
            inflation_max: f.inflation_max,
            recession_min: f.recession_min,
            recession_max: f.recession_max,
            iran_min: f.iran_min,
            iran_max: f.iran_max,
            risk_on_min: f.risk_on_min,
            risk_on_max: f.risk_on_max,
        }
    }
}

/// Per-(layer, topic) hit rate, used by `backtest layer-bias`.
#[derive(Debug, Clone, Serialize)]
pub struct LayerBiasRow {
    pub layer: String,
    pub topic: String,
    pub matched_predictions: usize,
    pub scored_predictions: usize,
    pub correct: usize,
    pub partial: usize,
    pub wrong: usize,
    pub hit_rate_pct: Option<f64>,
}

/// CLI args bundle for the scenario backtest.
#[derive(Debug, Clone, Default)]
pub struct ScenarioBacktestArgs<'a> {
    pub regime: Option<&'a str>,
    pub inflation_min: Option<f64>,
    pub inflation_max: Option<f64>,
    pub recession_min: Option<f64>,
    pub recession_max: Option<f64>,
    pub iran_min: Option<f64>,
    pub iran_max: Option<f64>,
    pub risk_on_min: Option<f64>,
    pub risk_on_max: Option<f64>,
    pub layer: Option<&'a str>,
    pub topic: Option<&'a str>,
    pub conviction: Option<&'a str>,
}

impl<'a> ScenarioBacktestArgs<'a> {
    /// Resolve `--regime` preset (if any), then merge per-flag overrides.
    pub fn resolve_filter(&self) -> Result<RegimeFilter> {
        let mut filter = match self.regime {
            Some(name) => regime_history::preset(name).ok_or_else(|| {
                anyhow!(
                    "unknown regime preset '{name}'; choose one of: {}",
                    regime_history::preset_names().join(", ")
                )
            })?,
            None => RegimeFilter::default(),
        };
        if let Some(v) = self.inflation_min {
            filter.inflation_min = Some(v);
        }
        if let Some(v) = self.inflation_max {
            filter.inflation_max = Some(v);
        }
        if let Some(v) = self.recession_min {
            filter.recession_min = Some(v);
        }
        if let Some(v) = self.recession_max {
            filter.recession_max = Some(v);
        }
        if let Some(v) = self.iran_min {
            filter.iran_min = Some(v);
        }
        if let Some(v) = self.iran_max {
            filter.iran_max = Some(v);
        }
        if let Some(v) = self.risk_on_min {
            filter.risk_on_min = Some(v);
        }
        if let Some(v) = self.risk_on_max {
            filter.risk_on_max = Some(v);
        }
        Ok(filter)
    }
}

/// Run `analytics backtest scenario`.
pub fn run_scenario(
    backend: &BackendConnection,
    args: ScenarioBacktestArgs<'_>,
    json_output: bool,
) -> Result<()> {
    let conn = backend
        .sqlite_native()
        .ok_or_else(|| anyhow!("scenario backtest currently requires the sqlite backend"))?;
    let summary = compute_scenario_backtest(conn, &args)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_scenario_summary(&summary);
    }
    Ok(())
}

/// Run `analytics backtest layer-bias`.
pub fn run_layer_bias(
    backend: &BackendConnection,
    args: ScenarioBacktestArgs<'_>,
    json_output: bool,
) -> Result<()> {
    let conn = backend
        .sqlite_native()
        .ok_or_else(|| anyhow!("layer-bias backtest currently requires the sqlite backend"))?;
    let filter = args.resolve_filter()?;
    let rows = compute_layer_bias(conn, &filter)?;
    let payload = json!({
        "regime": args.regime,
        "filter": SerializedFilter::from(filter),
        "rows": rows,
    });
    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_layer_bias(args.regime, &rows);
    }
    Ok(())
}

/// Compute the scenario backtest summary for a given filter + cohort.
pub fn compute_scenario_backtest(
    conn: &Connection,
    args: &ScenarioBacktestArgs<'_>,
) -> Result<ScenarioBacktestSummary> {
    let filter = args.resolve_filter()?;
    let predictions = load_matching_predictions(conn, &filter, args.layer, args.topic, args.conviction)?;

    let mut summary = ScenarioBacktestSummary {
        regime: args.regime.map(|s| s.to_string()),
        filter: filter.into(),
        layer: args.layer.map(|s| s.to_string()),
        topic: args.topic.map(|s| s.to_string()),
        conviction: args.conviction.map(|s| s.to_string()),
        ..Default::default()
    };
    summary.matched_predictions = predictions.len();
    for p in &predictions {
        match p.outcome.as_str() {
            "correct" => {
                summary.correct += 1;
                summary.scored_predictions += 1;
            }
            "partial" => {
                summary.partial += 1;
                summary.scored_predictions += 1;
            }
            "wrong" => {
                summary.wrong += 1;
                summary.scored_predictions += 1;
            }
            _ => summary.pending += 1,
        }
    }
    if summary.scored_predictions > 0 {
        // Match the rest of the codebase: count partial as half-credit.
        let weighted = summary.correct as f64 + (summary.partial as f64 * 0.5);
        let pct = 100.0 * weighted / summary.scored_predictions as f64;
        summary.hit_rate_pct = Some(round2(pct));
    }
    Ok(summary)
}

/// Compute one row per (layer × topic) cohort filtered by the regime.
pub fn compute_layer_bias(conn: &Connection, filter: &RegimeFilter) -> Result<Vec<LayerBiasRow>> {
    let predictions = load_matching_predictions(conn, filter, None, None, None)?;
    let mut buckets: BTreeMap<(String, String), LayerBiasRow> = BTreeMap::new();
    for p in predictions {
        let layer = p.timeframe.unwrap_or_else(|| "unknown".to_string()).to_lowercase();
        let topic = p.topic.to_lowercase();
        let row = buckets
            .entry((layer.clone(), topic.clone()))
            .or_insert(LayerBiasRow {
                layer,
                topic,
                matched_predictions: 0,
                scored_predictions: 0,
                correct: 0,
                partial: 0,
                wrong: 0,
                hit_rate_pct: None,
            });
        row.matched_predictions += 1;
        match p.outcome.as_str() {
            "correct" => {
                row.correct += 1;
                row.scored_predictions += 1;
            }
            "partial" => {
                row.partial += 1;
                row.scored_predictions += 1;
            }
            "wrong" => {
                row.wrong += 1;
                row.scored_predictions += 1;
            }
            _ => {}
        }
    }
    let mut rows: Vec<LayerBiasRow> = buckets
        .into_values()
        .map(|mut row| {
            if row.scored_predictions > 0 {
                let weighted = row.correct as f64 + (row.partial as f64 * 0.5);
                row.hit_rate_pct =
                    Some(round2(100.0 * weighted / row.scored_predictions as f64));
            }
            row
        })
        .collect();
    rows.sort_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then_with(|| a.topic.cmp(&b.topic))
    });
    Ok(rows)
}

/// A minimal prediction row used by the regime backtest. We deliberately
/// avoid `UserPrediction` here so the query can stay tight and tolerate any
/// future column drift.
#[derive(Debug, Clone)]
#[allow(dead_code)] // `id`/`conviction`/`symbol` retained for future drill-down JSON output
pub struct ScopedPrediction {
    pub id: i64,
    pub outcome: String,
    pub timeframe: Option<String>,
    pub topic: String,
    pub conviction: String,
    pub symbol: Option<String>,
}

/// Resolve the cohort of predictions whose scenario_prediction_links snapshot
/// satisfies the supplied filter. CLI-side `--layer/--topic/--conviction` are
/// applied on top.
pub fn load_matching_predictions(
    conn: &Connection,
    filter: &RegimeFilter,
    layer: Option<&str>,
    topic: Option<&str>,
    conviction: Option<&str>,
) -> Result<Vec<ScopedPrediction>> {
    // We need to detect whether scenario_prediction_links has a
    // `probability` column. In the current minimal schema it does not — only
    // the (scenario_id, prediction_id) pair. We fall back to "scenario's
    // current probability" via a join in that case.
    let has_probability = column_exists(conn, "scenario_prediction_links", "probability")?;
    let probability_expr = if has_probability {
        "spl.probability".to_string()
    } else {
        "s.probability".to_string()
    };

    // Build the per-scenario aggregate: for each prediction, find the
    // probability snapshot for each canonical scenario (inflation, recession,
    // iran, risk_on). We use scenario-name keyword matching identical to
    // `ScenarioState::from_probabilities`.
    let sql = format!(
        "SELECT DISTINCT up.id, up.outcome, up.timeframe, up.topic, up.conviction, up.symbol,
                MAX(CASE WHEN LOWER(s.name) LIKE '%inflation%' OR LOWER(s.name) LIKE '%stagflation%' THEN {prob} END) AS inflation,
                MAX(CASE WHEN LOWER(s.name) LIKE '%recession%' THEN {prob} END) AS recession,
                MAX(CASE WHEN LOWER(s.name) LIKE '%iran%' THEN {prob} END) AS iran,
                MAX(CASE WHEN LOWER(s.name) LIKE '%risk-on%' OR LOWER(s.name) LIKE '%risk on%' OR LOWER(s.name) LIKE '%soft landing%' THEN {prob} END) AS risk_on
         FROM user_predictions up
         JOIN scenario_prediction_links spl ON spl.prediction_id = up.id
         JOIN scenarios s ON s.id = spl.scenario_id
         GROUP BY up.id",
        prob = probability_expr,
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<f64>>(6)?,
            row.get::<_, Option<f64>>(7)?,
            row.get::<_, Option<f64>>(8)?,
            row.get::<_, Option<f64>>(9)?,
        ))
    })?;

    let mut out = Vec::new();
    for r in rows {
        let (id, outcome, timeframe, topic_val, conviction_val, symbol, infl, rec, iran, risk_on) =
            r?;
        let state = crate::db::regime_history::ScenarioState {
            inflation: infl,
            recession: rec,
            iran,
            risk_on,
            raw: BTreeMap::new(),
        };
        if !filter.matches(&state) {
            continue;
        }
        if let Some(want) = layer {
            if !timeframe
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case(want))
                .unwrap_or(false)
            {
                continue;
            }
        }
        if let Some(want) = topic {
            if !topic_val.eq_ignore_ascii_case(want) {
                continue;
            }
        }
        if let Some(want) = conviction {
            if !conviction_val.eq_ignore_ascii_case(want) {
                continue;
            }
        }
        out.push(ScopedPrediction {
            id,
            outcome,
            timeframe,
            topic: topic_val,
            conviction: conviction_val,
            symbol,
        });
    }
    Ok(out)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt =
        conn.prepare("SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2")?;
    let n: i64 = stmt.query_row(rusqlite::params![table, column], |row| row.get(0))?;
    Ok(n > 0)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn print_scenario_summary(s: &ScenarioBacktestSummary) {
    println!("Scenario-conditional backtest");
    if let Some(r) = &s.regime {
        println!("  Regime preset: {}", r);
    }
    if let Some(l) = &s.layer {
        println!("  Layer filter:  {}", l);
    }
    if let Some(t) = &s.topic {
        println!("  Topic filter:  {}", t);
    }
    if let Some(c) = &s.conviction {
        println!("  Conviction:    {}", c);
    }
    println!("  Matched predictions: {}", s.matched_predictions);
    println!(
        "  Scored: {} (correct {} / partial {} / wrong {})",
        s.scored_predictions, s.correct, s.partial, s.wrong
    );
    match s.hit_rate_pct {
        Some(p) => println!("  Hit rate: {:.2}%", p),
        None => println!("  Hit rate: n/a (no scored predictions in regime)"),
    }
}

fn print_layer_bias(regime: Option<&str>, rows: &[LayerBiasRow]) {
    println!("Layer-bias backtest");
    if let Some(r) = regime {
        println!("  Regime: {}", r);
    }
    if rows.is_empty() {
        println!("  (no predictions match)");
        return;
    }
    println!(
        "  {:<12} {:<14} {:>8} {:>8} {:>10}",
        "layer", "topic", "matched", "scored", "hit_rate%"
    );
    for row in rows {
        let hr = row
            .hit_rate_pct
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "n/a".to_string());
        println!(
            "  {:<12} {:<14} {:>8} {:>8} {:>10}",
            row.layer, row.topic, row.matched_predictions, row.scored_predictions, hr
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE scenarios (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                probability REAL NOT NULL DEFAULT 0,
                description TEXT,
                asset_impact TEXT,
                triggers TEXT,
                historical_precedent TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                phase TEXT NOT NULL DEFAULT 'active',
                resolved_at TEXT,
                resolution_notes TEXT
            );
            CREATE TABLE user_predictions (
                id INTEGER PRIMARY KEY,
                claim TEXT NOT NULL,
                symbol TEXT,
                conviction TEXT NOT NULL DEFAULT 'medium',
                timeframe TEXT NOT NULL DEFAULT 'medium',
                topic TEXT NOT NULL DEFAULT 'other',
                outcome TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE scenario_prediction_links (
                id INTEGER PRIMARY KEY,
                scenario_id INTEGER NOT NULL,
                prediction_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(scenario_id, prediction_id)
            );",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO scenarios (id, name, probability, status) VALUES
                (1, 'Inflation Spike', 90.0, 'active'),
                (2, 'Hard Recession', 25.0, 'active'),
                (3, 'Iran-US Escalation', 15.0, 'active'),
                (4, 'Risk-On Melt-Up', 20.0, 'active')",
            [],
        )
        .unwrap();

        // p1: LOW commodities, correct, satisfies stagflation-iran-cool
        conn.execute(
            "INSERT INTO user_predictions (id, claim, symbol, conviction, timeframe, topic, outcome) VALUES
                (1, 'gold up', 'GC=F', 'medium', 'low', 'commodities', 'correct'),
                (2, 'gold flat', 'GC=F', 'medium', 'low', 'commodities', 'wrong'),
                (3, 'spy up', 'SPY', 'high', 'high', 'equities', 'partial'),
                (4, 'btc rip', 'BTC-USD', 'medium', 'low', 'crypto', 'pending')",
            [],
        )
        .unwrap();

        // link all four predictions to all four scenarios
        for pid in 1..=4 {
            for sid in 1..=4 {
                conn.execute(
                    "INSERT INTO scenario_prediction_links (scenario_id, prediction_id) VALUES (?1, ?2)",
                    rusqlite::params![sid, pid],
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn stagflation_preset_filters_cohort_and_computes_hit_rate() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        let args = ScenarioBacktestArgs {
            regime: Some("stagflation-iran-cool"),
            ..Default::default()
        };
        let summary = compute_scenario_backtest(&conn, &args).unwrap();
        // All 4 predictions sit in a scenario state inflation=90, iran=15 →
        // matches stagflation-iran-cool.
        assert_eq!(summary.matched_predictions, 4);
        assert_eq!(summary.scored_predictions, 3);
        assert_eq!(summary.correct, 1);
        assert_eq!(summary.partial, 1);
        assert_eq!(summary.wrong, 1);
        // 1 correct + 0.5 partial out of 3 scored = 50%
        assert_eq!(summary.hit_rate_pct, Some(50.0));
    }

    #[test]
    fn layer_topic_filter_narrows_cohort() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        let args = ScenarioBacktestArgs {
            regime: Some("stagflation-iran-cool"),
            layer: Some("low"),
            topic: Some("commodities"),
            ..Default::default()
        };
        let summary = compute_scenario_backtest(&conn, &args).unwrap();
        assert_eq!(summary.matched_predictions, 2);
        assert_eq!(summary.scored_predictions, 2);
        assert_eq!(summary.correct, 1);
        assert_eq!(summary.wrong, 1);
        assert_eq!(summary.hit_rate_pct, Some(50.0));
    }

    #[test]
    fn unknown_regime_returns_error() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        let args = ScenarioBacktestArgs {
            regime: Some("does-not-exist"),
            ..Default::default()
        };
        let err = compute_scenario_backtest(&conn, &args).unwrap_err();
        assert!(err.to_string().contains("unknown regime preset"));
    }

    #[test]
    fn layer_bias_groups_by_layer_and_topic() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        let filter = regime_history::preset("stagflation-iran-cool").unwrap();
        let rows = compute_layer_bias(&conn, &filter).unwrap();
        // low/commodities has 2 predictions (1 correct, 1 wrong), high/equities 1 partial
        let low_comm = rows
            .iter()
            .find(|r| r.layer == "low" && r.topic == "commodities")
            .expect("low/commodities row present");
        assert_eq!(low_comm.matched_predictions, 2);
        assert_eq!(low_comm.hit_rate_pct, Some(50.0));
    }

    #[test]
    fn no_regime_no_filter_matches_everything() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        let args = ScenarioBacktestArgs::default();
        let summary = compute_scenario_backtest(&conn, &args).unwrap();
        assert_eq!(summary.matched_predictions, 4);
    }
}
