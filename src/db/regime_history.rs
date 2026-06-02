#![allow(dead_code)] // Some accessors are exposed for future TUI / report-skill consumers.

//! Regime history — daily classification of the current scenario regime.
//!
//! Each row records, for a given UTC date, which named regime preset (or the
//! literal "neutral") matched the latest probability of every scenario at the
//! time the classifier ran. The full probability snapshot is stored as JSON so
//! later analytics can replay the classification logic without re-querying
//! `scenario_history`.
//!
//! Schema (sqlite — postgres is not yet wired):
//!
//! ```text
//! CREATE TABLE IF NOT EXISTS regime_history (
//!     date TEXT PRIMARY KEY,
//!     regime TEXT NOT NULL,
//!     scenario_state_json TEXT NOT NULL,
//!     classified_at TEXT NOT NULL DEFAULT (datetime('now'))
//! );
//! ```
//!
//! The classifier is idempotent: `record_today_backend` is safe to invoke
//! more than once per day. The most recent classification for a date wins via
//! `INSERT ... ON CONFLICT(date) DO UPDATE`.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::backend::BackendConnection;

/// Canonical regime presets, in priority order.
///
/// The first preset whose filter matches the current scenario state wins.
/// If no preset matches, the regime is recorded as `"neutral"`.
///
/// **Definitions** (documented in `AGENTS.md` and the TODO that introduced
/// this module):
///
/// - `stagflation-iran-cool` — Inflation Spike ≥ 85 AND Iran ≤ 20.
/// - `crisis` — Hard Recession ≥ 40 AND Iran ≥ 30.
/// - `risk-on` — Risk-On ≥ 40.
///
/// Each preset's filter is expressed as a `RegimeFilter` struct so the
/// `backtest` CLI can re-use exactly the same thresholds without divergence.
pub const PRESETS: &[(&str, RegimeFilter)] = &[
    (
        "stagflation-iran-cool",
        RegimeFilter {
            inflation_min: Some(85.0),
            inflation_max: None,
            recession_min: None,
            recession_max: None,
            iran_min: None,
            iran_max: Some(20.0),
            risk_on_min: None,
            risk_on_max: None,
        },
    ),
    (
        "crisis",
        RegimeFilter {
            inflation_min: None,
            inflation_max: None,
            recession_min: Some(40.0),
            recession_max: None,
            iran_min: Some(30.0),
            iran_max: None,
            risk_on_min: None,
            risk_on_max: None,
        },
    ),
    (
        "risk-on",
        RegimeFilter {
            inflation_min: None,
            inflation_max: None,
            recession_min: None,
            recession_max: None,
            iran_min: None,
            iran_max: None,
            risk_on_min: Some(40.0),
            risk_on_max: None,
        },
    ),
];

/// Filter describing a regime preset by per-scenario probability bands.
///
/// Each field defaults to `None`, meaning "no constraint". A scenario state
/// matches the filter only when *every* `Some(_)` band is satisfied.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RegimeFilter {
    pub inflation_min: Option<f64>,
    pub inflation_max: Option<f64>,
    pub recession_min: Option<f64>,
    pub recession_max: Option<f64>,
    pub iran_min: Option<f64>,
    pub iran_max: Option<f64>,
    pub risk_on_min: Option<f64>,
    pub risk_on_max: Option<f64>,
}

impl RegimeFilter {
    /// Returns true when the supplied scenario state satisfies every band.
    pub fn matches(&self, state: &ScenarioState) -> bool {
        let pairs: &[(Option<f64>, Option<f64>, Option<f64>)] = &[
            (self.inflation_min, self.inflation_max, state.inflation),
            (self.recession_min, self.recession_max, state.recession),
            (self.iran_min, self.iran_max, state.iran),
            (self.risk_on_min, self.risk_on_max, state.risk_on),
        ];
        for (min, max, value) in pairs {
            match (min, max, value) {
                (Some(lo), _, Some(v)) if v < lo => return false,
                (Some(_), _, None) => return false,
                (_, Some(hi), Some(v)) if v > hi => return false,
                (_, Some(_), None) => return false,
                _ => {}
            }
        }
        true
    }

    /// True when at least one band is set.
    pub fn is_constrained(&self) -> bool {
        self.inflation_min.is_some()
            || self.inflation_max.is_some()
            || self.recession_min.is_some()
            || self.recession_max.is_some()
            || self.iran_min.is_some()
            || self.iran_max.is_some()
            || self.risk_on_min.is_some()
            || self.risk_on_max.is_some()
    }
}

/// Resolved current state of the four canonical regime-classifying scenarios.
///
/// Missing values stay `None` so that a regime which depends on a scenario
/// that has never been recorded simply fails to match (rather than silently
/// treating it as 0).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioState {
    pub inflation: Option<f64>,
    pub recession: Option<f64>,
    pub iran: Option<f64>,
    pub risk_on: Option<f64>,
    /// All scenario probabilities keyed by canonical (lower-case) scenario
    /// name — retained so the JSON column can be replayed without losing
    /// fidelity. Iteration order is deterministic (BTreeMap).
    pub raw: BTreeMap<String, f64>,
}

impl ScenarioState {
    pub fn from_probabilities(raw: BTreeMap<String, f64>) -> Self {
        let inflation = pick(&raw, &["inflation spike", "inflation", "stagflation"]);
        let recession = pick(&raw, &["hard recession", "recession"]);
        let iran = pick(&raw, &["iran-us", "iran us", "iran", "iran conflict"]);
        let risk_on = pick(&raw, &["risk-on", "risk on", "soft landing"]);
        Self {
            inflation,
            recession,
            iran,
            risk_on,
            raw,
        }
    }
}

fn pick(raw: &BTreeMap<String, f64>, candidates: &[&str]) -> Option<f64> {
    for c in candidates {
        let needle = c.to_lowercase();
        for (key, v) in raw.iter() {
            if key.contains(&needle) {
                return Some(*v);
            }
        }
    }
    None
}

/// Classify a scenario state against the canonical preset list. Returns the
/// first preset whose filter matches, falling back to `"neutral"`.
pub fn classify(state: &ScenarioState) -> &'static str {
    for (name, filter) in PRESETS {
        if filter.matches(state) {
            return name;
        }
    }
    "neutral"
}

/// Look up a preset filter by name (case-insensitive).
pub fn preset(name: &str) -> Option<RegimeFilter> {
    let needle = name.to_lowercase();
    PRESETS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&needle))
        .map(|(_, f)| *f)
}

/// All preset names.
pub fn preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|(n, _)| *n).collect()
}

/// A persisted regime classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeHistoryRow {
    pub date: String,
    pub regime: String,
    pub scenario_state_json: String,
    pub classified_at: String,
}

/// Idempotently create the `regime_history` table.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS regime_history (
            date TEXT PRIMARY KEY,
            regime TEXT NOT NULL,
            scenario_state_json TEXT NOT NULL,
            classified_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_regime_history_regime
            ON regime_history(regime);",
    )?;
    Ok(())
}

/// Read the latest probability for every active scenario, keyed by
/// lower-cased scenario name.
pub fn latest_scenario_probabilities(conn: &Connection) -> Result<BTreeMap<String, f64>> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT s.name, COALESCE(
                (SELECT h.probability
                 FROM scenario_history h
                 WHERE h.scenario_id = s.id
                 ORDER BY h.id DESC
                 LIMIT 1),
                s.probability
            )
         FROM scenarios s
         WHERE COALESCE(s.status, '') != 'system'",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let prob: f64 = row.get(1)?;
        out.insert(name.to_lowercase(), prob);
    }
    Ok(out)
}

/// Resolve the current scenario state from the live DB.
pub fn current_state(conn: &Connection) -> Result<ScenarioState> {
    ensure_table(conn)?;
    let raw = latest_scenario_probabilities(conn)?;
    Ok(ScenarioState::from_probabilities(raw))
}

/// Insert (or replace) the classification for the given UTC date.
pub fn record(conn: &Connection, date: &str, state: &ScenarioState) -> Result<String> {
    ensure_table(conn)?;
    let regime = classify(state).to_string();
    let json = serde_json::to_string(state)?;
    let classified_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO regime_history (date, regime, scenario_state_json, classified_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date) DO UPDATE SET
            regime = excluded.regime,
            scenario_state_json = excluded.scenario_state_json,
            classified_at = excluded.classified_at",
        params![date, regime, json, classified_at],
    )?;
    Ok(regime)
}

/// Classify and record today's regime. Backend-aware shim — currently a
/// no-op for postgres backends (the refresh hook simply skips).
pub fn record_today_backend(backend: &BackendConnection) -> Result<Option<String>> {
    if let Some(conn) = backend.sqlite_native() {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let state = current_state(conn)?;
        Ok(Some(record(conn, &date, &state)?))
    } else {
        Ok(None)
    }
}

/// Fetch the most recently classified regime, if any.
pub fn latest(conn: &Connection) -> Result<Option<RegimeHistoryRow>> {
    ensure_table(conn)?;
    let row = conn
        .query_row(
            "SELECT date, regime, scenario_state_json, classified_at
             FROM regime_history
             ORDER BY date DESC
             LIMIT 1",
            [],
            |row| {
                Ok(RegimeHistoryRow {
                    date: row.get(0)?,
                    regime: row.get(1)?,
                    scenario_state_json: row.get(2)?,
                    classified_at: row.get(3)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Backend-aware accessor.
pub fn latest_backend(backend: &BackendConnection) -> Result<Option<RegimeHistoryRow>> {
    if let Some(conn) = backend.sqlite_native() {
        latest(conn)
    } else {
        Ok(None)
    }
}

/// Return all dates whose classification matches the given regime.
pub fn dates_for_regime(conn: &Connection, regime: &str) -> Result<Vec<String>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT date FROM regime_history WHERE regime = ?1 ORDER BY date ASC",
    )?;
    let mut rows = stmt.query(params![regime])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row.get::<_, String>(0)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(infl: Option<f64>, rec: Option<f64>, iran: Option<f64>, risk_on: Option<f64>) -> ScenarioState {
        ScenarioState {
            inflation: infl,
            recession: rec,
            iran,
            risk_on,
            raw: BTreeMap::new(),
        }
    }

    #[test]
    fn stagflation_iran_cool_matches_inflation_high_iran_low() {
        let s = state(Some(90.0), Some(20.0), Some(15.0), Some(10.0));
        assert_eq!(classify(&s), "stagflation-iran-cool");
    }

    #[test]
    fn stagflation_iran_cool_rejected_when_iran_too_hot() {
        // Inflation high but iran above the 20 ceiling — falls through to neutral
        let s = state(Some(90.0), Some(20.0), Some(35.0), Some(10.0));
        assert_ne!(classify(&s), "stagflation-iran-cool");
    }

    #[test]
    fn crisis_matches_recession_high_iran_high() {
        let s = state(Some(60.0), Some(50.0), Some(35.0), Some(5.0));
        // Inflation 60 doesn't trip stagflation-iran-cool (iran 35 > 20).
        assert_eq!(classify(&s), "crisis");
    }

    #[test]
    fn risk_on_matches_when_risk_on_above_threshold() {
        let s = state(Some(30.0), Some(10.0), Some(15.0), Some(55.0));
        assert_eq!(classify(&s), "risk-on");
    }

    #[test]
    fn neutral_when_no_preset_matches() {
        let s = state(Some(20.0), Some(20.0), Some(25.0), Some(20.0));
        assert_eq!(classify(&s), "neutral");
    }

    #[test]
    fn missing_scenarios_fail_constrained_filters_gracefully() {
        // Missing all scenarios should never match a constrained filter.
        let s = state(None, None, None, None);
        assert_eq!(classify(&s), "neutral");
    }

    #[test]
    fn preset_lookup_is_case_insensitive() {
        assert!(preset("STAGFLATION-IRAN-COOL").is_some());
        assert!(preset("Crisis").is_some());
        assert!(preset("unknown").is_none());
    }

    #[test]
    fn scenario_state_from_probabilities_picks_canonical_names() {
        let mut raw = BTreeMap::new();
        raw.insert("inflation spike".to_string(), 85.0);
        raw.insert("hard recession".to_string(), 25.0);
        raw.insert("iran-us escalation".to_string(), 15.0);
        raw.insert("risk-on melt-up".to_string(), 30.0);
        let s = ScenarioState::from_probabilities(raw);
        assert_eq!(s.inflation, Some(85.0));
        assert_eq!(s.recession, Some(25.0));
        assert_eq!(s.iran, Some(15.0));
        assert_eq!(s.risk_on, Some(30.0));
    }

    #[test]
    fn record_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let s = state(Some(90.0), None, Some(15.0), None);
        let r1 = record(&conn, "2026-06-02", &s).unwrap();
        let r2 = record(&conn, "2026-06-02", &s).unwrap();
        assert_eq!(r1, r2);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM regime_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
