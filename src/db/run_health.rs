//! run_health — per-run epistemic-health instrumentation (epistemics R4).
//!
//! One row per report run (keyed by `run_date`) recording how healthy the
//! multi-agent intelligence process itself was:
//!
//!   - agreement_rate       share of voices agreeing with the operator stance (0-1)
//!   - blind_divergence     mean |house conviction − blind conviction| across held assets
//!   - panel_dispersion     stddev of panel persona confidences
//!   - novelty_rate         share of this run's notes that are novel (R5 computes)
//!   - fallback_warnings    count of empty-state fallbacks hit during the run
//!   - scenario_delta_total sum |Δprobability| across scenarios today
//!   - audit_pass_rate      accuracy-audit claims_passed/claims_total
//!   - agents_spawned       agents launched during the run
//!
//! The report-skill orchestrator records what it computes; Rust derives what
//! it can on its own (blind_divergence from same-day `analyst_view_history`,
//! scenario_delta_total from the `scenario_updates` probability ledger).
//!
//! SQLite-only module: this is local run instrumentation, mirrored after the
//! prediction-preflight precedent (commands bail on the Postgres backend).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::analyst_views::{effective_conviction, is_canonical_analyst};

/// Threshold above which the run is flagged as echo-risk.
pub const AGREEMENT_ECHO_THRESHOLD: f64 = 0.85;
/// Threshold below which the panel is flagged as persona-washed.
pub const PANEL_DISPERSION_FLOOR: f64 = 4.0;
/// Threshold above which the house view is flagged as far from the raw-data read.
pub const BLIND_DIVERGENCE_CEILING: f64 = 2.0;

/// One run's epistemic-health row.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunHealth {
    pub id: i64,
    pub run_date: String,
    pub agreement_rate: Option<f64>,
    pub blind_divergence: Option<f64>,
    pub panel_dispersion: Option<f64>,
    pub novelty_rate: Option<f64>,
    pub fallback_warnings: Option<i64>,
    pub scenario_delta_total: Option<f64>,
    pub audit_pass_rate: Option<f64>,
    pub agents_spawned: Option<i64>,
    pub notes: Option<String>,
    pub created_at: String,
}

/// Metrics payload for an upsert. All fields optional — provided values win,
/// omitted values keep whatever an earlier record call stored (incremental
/// recording across a run is expected: e.g. novelty lands later than
/// agreement).
#[derive(Debug, Clone, Default)]
pub struct RunHealthInput {
    pub agreement_rate: Option<f64>,
    pub blind_divergence: Option<f64>,
    pub panel_dispersion: Option<f64>,
    pub novelty_rate: Option<f64>,
    pub fallback_warnings: Option<i64>,
    pub scenario_delta_total: Option<f64>,
    pub audit_pass_rate: Option<f64>,
    pub agents_spawned: Option<i64>,
    pub notes: Option<String>,
}

fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS run_health (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_date TEXT NOT NULL,
            agreement_rate REAL,
            blind_divergence REAL,
            panel_dispersion REAL,
            novelty_rate REAL,
            fallback_warnings INTEGER,
            scenario_delta_total REAL,
            audit_pass_rate REAL,
            agents_spawned INTEGER,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_run_health_run_date
            ON run_health(run_date);",
    )?;
    Ok(())
}

/// Upsert the run-health row for a date. Field-wise merge: provided values
/// overwrite, omitted values keep the previously recorded value.
pub fn upsert_run_health(conn: &Connection, run_date: &str, input: &RunHealthInput) -> Result<i64> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO run_health
            (run_date, agreement_rate, blind_divergence, panel_dispersion, novelty_rate,
             fallback_warnings, scenario_delta_total, audit_pass_rate, agents_spawned, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(run_date) DO UPDATE SET
            agreement_rate = COALESCE(excluded.agreement_rate, run_health.agreement_rate),
            blind_divergence = COALESCE(excluded.blind_divergence, run_health.blind_divergence),
            panel_dispersion = COALESCE(excluded.panel_dispersion, run_health.panel_dispersion),
            novelty_rate = COALESCE(excluded.novelty_rate, run_health.novelty_rate),
            fallback_warnings = COALESCE(excluded.fallback_warnings, run_health.fallback_warnings),
            scenario_delta_total = COALESCE(excluded.scenario_delta_total, run_health.scenario_delta_total),
            audit_pass_rate = COALESCE(excluded.audit_pass_rate, run_health.audit_pass_rate),
            agents_spawned = COALESCE(excluded.agents_spawned, run_health.agents_spawned),
            notes = COALESCE(excluded.notes, run_health.notes)",
        params![
            run_date,
            input.agreement_rate,
            input.blind_divergence,
            input.panel_dispersion,
            input.novelty_rate,
            input.fallback_warnings,
            input.scenario_delta_total,
            input.audit_pass_rate,
            input.agents_spawned,
            input.notes,
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM run_health WHERE run_date = ?1",
        params![run_date],
        |row| row.get(0),
    )?;
    Ok(id)
}

fn row_to_run_health(row: &rusqlite::Row) -> Result<RunHealth, rusqlite::Error> {
    Ok(RunHealth {
        id: row.get(0)?,
        run_date: row.get(1)?,
        agreement_rate: row.get(2)?,
        blind_divergence: row.get(3)?,
        panel_dispersion: row.get(4)?,
        novelty_rate: row.get(5)?,
        fallback_warnings: row.get(6)?,
        scenario_delta_total: row.get(7)?,
        audit_pass_rate: row.get(8)?,
        agents_spawned: row.get(9)?,
        notes: row.get(10)?,
        created_at: row.get(11)?,
    })
}

const RUN_HEALTH_COLUMNS: &str = "id, run_date, agreement_rate, blind_divergence, \
     panel_dispersion, novelty_rate, fallback_warnings, scenario_delta_total, \
     audit_pass_rate, agents_spawned, notes, created_at";

/// Fetch one run's row by date.
pub fn get_run_health(conn: &Connection, run_date: &str) -> Result<Option<RunHealth>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_HEALTH_COLUMNS} FROM run_health WHERE run_date = ?1"
    ))?;
    let mut rows = stmt.query_map(params![run_date], row_to_run_health)?;
    match rows.next() {
        Some(Ok(r)) => Ok(Some(r)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

/// Fetch the most recently dated run row.
pub fn get_latest_run_health(conn: &Connection) -> Result<Option<RunHealth>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_HEALTH_COLUMNS} FROM run_health ORDER BY run_date DESC LIMIT 1"
    ))?;
    let mut rows = stmt.query_map([], row_to_run_health)?;
    match rows.next() {
        Some(Ok(r)) => Ok(Some(r)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

/// Newest-first trend rows.
pub fn list_run_health(conn: &Connection, limit: usize) -> Result<Vec<RunHealth>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_HEALTH_COLUMNS} FROM run_health ORDER BY run_date DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map(params![limit as i64], row_to_run_health)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Threshold flags for a row. Returned as (metric, warning) pairs so both the
/// CLI and the report section render identically.
pub fn threshold_flags(row: &RunHealth) -> Vec<(&'static str, String)> {
    let mut flags = Vec::new();
    if let Some(a) = row.agreement_rate {
        if a > AGREEMENT_ECHO_THRESHOLD {
            flags.push((
                "agreement_rate",
                format!("⚠ echo risk ({:.2} > {:.2})", a, AGREEMENT_ECHO_THRESHOLD),
            ));
        }
    }
    if let Some(p) = row.panel_dispersion {
        if p < PANEL_DISPERSION_FLOOR {
            flags.push((
                "panel_dispersion",
                format!(
                    "⚠ persona washing ({:.1} < {:.1})",
                    p, PANEL_DISPERSION_FLOOR
                ),
            ));
        }
    }
    if let Some(b) = row.blind_divergence {
        if b > BLIND_DIVERGENCE_CEILING {
            flags.push((
                "blind_divergence",
                format!(
                    "⚠ house view far from raw-data read ({:.1} > {:.1})",
                    b, BLIND_DIVERGENCE_CEILING
                ),
            ));
        }
    }
    flags
}

/// Derive blind divergence for a day from `analyst_view_history`: for every
/// asset where the blind layer wrote a view that day AND at least one
/// canonical layer did too, take |mean(canonical convictions) − blind
/// conviction| (direction-authoritative signs), then average across assets.
/// Returns `None` when no asset qualifies (e.g. no blind run that day).
pub fn compute_blind_divergence(conn: &Connection, run_date: &str) -> Result<Option<f64>> {
    // analyst_view_history is created lazily by the analyst-views module;
    // a database that has never recorded a view has nothing to derive.
    let table_exists: i64 = conn
        .prepare(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'analyst_view_history'",
        )?
        .query_row([], |row| row.get(0))
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT analyst, asset, direction, conviction, recorded_at
         FROM analyst_view_history
         WHERE date(recorded_at) = ?1
         ORDER BY recorded_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![run_date], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    // Latest same-day view per (asset, analyst); ascending scan means later
    // rows overwrite earlier ones.
    let mut latest: std::collections::BTreeMap<(String, String), i64> =
        std::collections::BTreeMap::new();
    for r in rows {
        let (analyst, asset, direction, conviction) = r?;
        latest.insert(
            (asset.to_uppercase(), analyst.clone()),
            effective_conviction(&direction, conviction),
        );
    }

    let mut per_asset: std::collections::BTreeMap<String, (Vec<i64>, Option<i64>)> =
        std::collections::BTreeMap::new();
    for ((asset, analyst), conviction) in latest {
        let entry = per_asset.entry(asset).or_default();
        if is_canonical_analyst(&analyst) {
            entry.0.push(conviction);
        } else if analyst == "blind" {
            entry.1 = Some(conviction);
        }
    }

    let mut diffs = Vec::new();
    for (_asset, (canonical, blind)) in per_asset {
        let (Some(blind_conv), false) = (blind, canonical.is_empty()) else {
            continue;
        };
        let mean = canonical.iter().sum::<i64>() as f64 / canonical.len() as f64;
        diffs.push((mean - blind_conv as f64).abs());
    }
    if diffs.is_empty() {
        return Ok(None);
    }
    let avg = diffs.iter().sum::<f64>() / diffs.len() as f64;
    Ok(Some((avg * 100.0).round() / 100.0))
}

/// One agent's scored-prediction track record (rivalry scoreboard row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RivalryRow {
    pub source_agent: String,
    /// `rival` (analyst-antithesis), `house` (analyst-*), or `other`.
    pub camp: String,
    pub scored: i64,
    pub correct: i64,
    pub wrong: i64,
    pub partial: i64,
    /// correct / (correct + wrong + partial), percent, 1 decimal. None when
    /// no prediction has a definitive outcome yet.
    pub hit_rate_pct: Option<f64>,
}

/// House-vs-rival scoreboard payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RivalryReport {
    pub rows: Vec<RivalryRow>,
    /// Pending (unscored) predictions still accruing on the antithesis ledger.
    pub antithesis_pending: i64,
}

fn camp_for(agent: &str) -> &'static str {
    if agent == "analyst-antithesis" {
        "rival"
    } else if agent.starts_with("analyst-") {
        "house"
    } else {
        "other"
    }
}

/// Compute the rivalry scoreboard from `user_predictions`.
pub fn compute_rivalry(conn: &Connection) -> Result<RivalryReport> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(source_agent, '(unspecified)') AS agent, outcome, COUNT(*)
         FROM user_predictions
         WHERE outcome != 'pending'
         GROUP BY agent, outcome",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut by_agent: std::collections::BTreeMap<String, (i64, i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for r in rows {
        let (agent, outcome, n) = r?;
        let entry = by_agent.entry(agent).or_default();
        entry.0 += n; // scored total
        match outcome.as_str() {
            "correct" => entry.1 += n,
            "wrong" => entry.2 += n,
            "partial" => entry.3 += n,
            _ => {}
        }
    }

    let mut out: Vec<RivalryRow> = by_agent
        .into_iter()
        .map(|(agent, (scored, correct, wrong, partial))| {
            let definitive = correct + wrong + partial;
            let hit_rate_pct = if definitive > 0 {
                Some((correct as f64 / definitive as f64 * 1000.0).round() / 10.0)
            } else {
                None
            };
            RivalryRow {
                camp: camp_for(&agent).to_string(),
                source_agent: agent,
                scored,
                correct,
                wrong,
                partial,
                hit_rate_pct,
            }
        })
        .collect();
    // Rival first, then house layers by hit rate desc, then the rest.
    out.sort_by(|a, b| {
        let rank = |r: &RivalryRow| match r.camp.as_str() {
            "rival" => 0,
            "house" => 1,
            _ => 2,
        };
        rank(a).cmp(&rank(b)).then(
            b.hit_rate_pct
                .unwrap_or(-1.0)
                .partial_cmp(&a.hit_rate_pct.unwrap_or(-1.0))
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    let antithesis_pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM user_predictions
         WHERE source_agent = 'analyst-antithesis' AND outcome = 'pending'",
        [],
        |row| row.get(0),
    )?;

    Ok(RivalryReport {
        rows: out,
        antithesis_pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        crate::db::open_in_memory()
    }

    #[test]
    fn upsert_is_keyed_by_date_and_merges_fields() {
        let conn = setup();
        let id1 = upsert_run_health(
            &conn,
            "2026-06-10",
            &RunHealthInput {
                agreement_rate: Some(0.9),
                ..Default::default()
            },
        )
        .unwrap();
        // Second record for the same date with a different metric merges.
        let id2 = upsert_run_health(
            &conn,
            "2026-06-10",
            &RunHealthInput {
                novelty_rate: Some(0.4),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(id1, id2);
        let row = get_run_health(&conn, "2026-06-10").unwrap().unwrap();
        assert_eq!(row.agreement_rate, Some(0.9));
        assert_eq!(row.novelty_rate, Some(0.4));

        upsert_run_health(
            &conn,
            "2026-06-11",
            &RunHealthInput {
                agreement_rate: Some(0.5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(list_run_health(&conn, 10).unwrap().len(), 2);
        let latest = get_latest_run_health(&conn).unwrap().unwrap();
        assert_eq!(latest.run_date, "2026-06-11");
    }

    #[test]
    fn threshold_flags_fire_on_breaches_only() {
        let healthy = RunHealth {
            agreement_rate: Some(0.7),
            panel_dispersion: Some(6.0),
            blind_divergence: Some(1.0),
            ..Default::default()
        };
        assert!(threshold_flags(&healthy).is_empty());

        let sick = RunHealth {
            agreement_rate: Some(0.95),
            panel_dispersion: Some(2.0),
            blind_divergence: Some(3.5),
            ..Default::default()
        };
        let flags = threshold_flags(&sick);
        assert_eq!(flags.len(), 3);
        assert!(flags.iter().any(|(_, w)| w.contains("echo risk")));
        assert!(flags.iter().any(|(_, w)| w.contains("persona washing")));
        assert!(flags.iter().any(|(_, w)| w.contains("raw-data read")));
    }

    /// analyst_view_history is lazily created by the analyst-views module;
    /// tests create the minimal shape directly.
    fn ensure_history_table(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS analyst_view_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL,
                key_evidence TEXT,
                blind_spots TEXT,
                allocation_bias TEXT,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
    }

    #[test]
    fn blind_divergence_derives_from_same_day_views() {
        let conn = setup();
        ensure_history_table(&conn);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        // Canonical layers at +4/+2 (mean 3.0); blind at -1 → diff 4.0.
        for (analyst, direction, conviction) in [
            ("low", "bull", 4i64),
            ("medium", "bull", 2),
            ("blind", "bear", -1),
        ] {
            conn.execute(
                "INSERT INTO analyst_view_history
                    (analyst, asset, direction, conviction, reasoning_summary)
                 VALUES (?, 'BTC', ?, ?, 'r')",
                params![analyst, direction, conviction],
            )
            .unwrap();
        }
        let d = compute_blind_divergence(&conn, &today).unwrap();
        assert_eq!(d, Some(4.0));
    }

    #[test]
    fn blind_divergence_none_without_blind_rows() {
        let conn = setup();
        ensure_history_table(&conn);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        conn.execute(
            "INSERT INTO analyst_view_history
                (analyst, asset, direction, conviction, reasoning_summary)
             VALUES ('low', 'BTC', 'bull', 3, 'r')",
            [],
        )
        .unwrap();
        assert_eq!(compute_blind_divergence(&conn, &today).unwrap(), None);
    }

    #[test]
    fn blind_divergence_none_when_table_missing() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        assert_eq!(
            compute_blind_divergence(&conn, "2026-06-10").unwrap(),
            None
        );
    }

    #[test]
    fn rivalry_groups_by_source_agent() {
        let conn = setup();
        for (agent, outcome) in [
            ("analyst-antithesis", "correct"),
            ("analyst-antithesis", "wrong"),
            ("analyst-antithesis", "pending"),
            ("analyst-medium", "correct"),
            ("analyst-medium", "correct"),
            ("analyst-medium", "partial"),
        ] {
            conn.execute(
                "INSERT INTO user_predictions (claim, source_agent, outcome)
                 VALUES ('test claim', ?, ?)",
                params![agent, outcome],
            )
            .unwrap();
        }
        let report = compute_rivalry(&conn).unwrap();
        assert_eq!(report.antithesis_pending, 1);
        let rival = report
            .rows
            .iter()
            .find(|r| r.source_agent == "analyst-antithesis")
            .unwrap();
        assert_eq!(rival.camp, "rival");
        assert_eq!(rival.scored, 2);
        assert_eq!(rival.hit_rate_pct, Some(50.0));
        let house = report
            .rows
            .iter()
            .find(|r| r.source_agent == "analyst-medium")
            .unwrap();
        assert_eq!(house.camp, "house");
        assert_eq!(house.scored, 3);
        assert_eq!(house.hit_rate_pct, Some(66.7));
    }
}
