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
/// |Pearson r| between a layer's conviction trajectory and the asset's price
/// above which conviction is flagged as momentum dressed as structure
/// (standing rule 15: conviction must not track price).
pub const CONVICTION_PRICE_CORR_CEILING: f64 = 0.6;
/// Minimum paired (conviction, close) observations for a correlation read.
pub const CONVICTION_PRICE_MIN_N: usize = 6;

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
    /// Max |Pearson r| between any canonical layer's conviction trajectory
    /// and the matching held asset's closes over the trailing window.
    /// > 0.6 flags "momentum dressed as structure" (standing rule 15).
    pub conviction_price_corr: Option<f64>,
    /// Overall scored direction-hit rate over the trailing 30d of
    /// `forecast_scores` (non-neutral scored cells). Self-derived by
    /// `analytics epistemics record` when the flag is omitted.
    pub forecast_hit_rate: Option<f64>,
    /// Count of ACTIVE `forecast_misalignments` rows at record time.
    /// > 0 flags the run — probation is in force somewhere.
    pub active_misalignments: Option<i64>,
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
    pub conviction_price_corr: Option<f64>,
    pub forecast_hit_rate: Option<f64>,
    pub active_misalignments: Option<i64>,
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
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            conviction_price_corr REAL,
            forecast_hit_rate REAL,
            active_misalignments INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_run_health_run_date
            ON run_health(run_date);",
    )?;
    // Additive migrations: columns appended after the table first shipped.
    // Idempotent via pragma_table_info; appended last so legacy and fresh
    // tables share column order.
    //   conviction_price_corr — gold post-mortem T2
    //   forecast_hit_rate / active_misalignments — misalignment tripwires (R2)
    for (column, kind) in [
        ("conviction_price_corr", "REAL"),
        ("forecast_hit_rate", "REAL"),
        ("active_misalignments", "INTEGER"),
    ] {
        let exists: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('run_health') WHERE name = ?1")?
            .query_row(params![column], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute_batch(&format!(
                "ALTER TABLE run_health ADD COLUMN {column} {kind}"
            ))?;
        }
    }
    Ok(())
}

/// Upsert the run-health row for a date. Field-wise merge: provided values
/// overwrite, omitted values keep the previously recorded value.
pub fn upsert_run_health(conn: &Connection, run_date: &str, input: &RunHealthInput) -> Result<i64> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO run_health
            (run_date, agreement_rate, blind_divergence, panel_dispersion, novelty_rate,
             fallback_warnings, scenario_delta_total, audit_pass_rate, agents_spawned, notes,
             conviction_price_corr, forecast_hit_rate, active_misalignments)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(run_date) DO UPDATE SET
            agreement_rate = COALESCE(excluded.agreement_rate, run_health.agreement_rate),
            blind_divergence = COALESCE(excluded.blind_divergence, run_health.blind_divergence),
            panel_dispersion = COALESCE(excluded.panel_dispersion, run_health.panel_dispersion),
            novelty_rate = COALESCE(excluded.novelty_rate, run_health.novelty_rate),
            fallback_warnings = COALESCE(excluded.fallback_warnings, run_health.fallback_warnings),
            scenario_delta_total = COALESCE(excluded.scenario_delta_total, run_health.scenario_delta_total),
            audit_pass_rate = COALESCE(excluded.audit_pass_rate, run_health.audit_pass_rate),
            agents_spawned = COALESCE(excluded.agents_spawned, run_health.agents_spawned),
            notes = COALESCE(excluded.notes, run_health.notes),
            conviction_price_corr = COALESCE(excluded.conviction_price_corr, run_health.conviction_price_corr),
            forecast_hit_rate = COALESCE(excluded.forecast_hit_rate, run_health.forecast_hit_rate),
            active_misalignments = COALESCE(excluded.active_misalignments, run_health.active_misalignments)",
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
            input.conviction_price_corr,
            input.forecast_hit_rate,
            input.active_misalignments,
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
        conviction_price_corr: row.get(12)?,
        forecast_hit_rate: row.get(13)?,
        active_misalignments: row.get(14)?,
    })
}

const RUN_HEALTH_COLUMNS: &str = "id, run_date, agreement_rate, blind_divergence, \
     panel_dispersion, novelty_rate, fallback_warnings, scenario_delta_total, \
     audit_pass_rate, agents_spawned, notes, created_at, conviction_price_corr, \
     forecast_hit_rate, active_misalignments";

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
    if let Some(c) = row.conviction_price_corr {
        if c.abs() > CONVICTION_PRICE_CORR_CEILING {
            flags.push((
                "conviction_price_corr",
                format!(
                    "⚠ momentum dressed as structure (standing rule 15) (|r| {:.2} > {:.2})",
                    c.abs(),
                    CONVICTION_PRICE_CORR_CEILING
                ),
            ));
        }
    }
    if let Some(n) = row.active_misalignments {
        if n > 0 {
            flags.push((
                "active_misalignments",
                format!(
                    "⚠ {n} active forecast misalignment(s) — probation in force (`pftui research misalignments`)"
                ),
            ));
        }
    }
    flags
}

/// Overall scored direction-hit rate (0..1) over the trailing `window_days`
/// of `forecast_scores` (non-neutral scored cells, all layers/horizons).
/// `None` when the table doesn't exist yet or nothing scored in the window.
/// Self-derived into `run_health.forecast_hit_rate` by
/// `analytics epistemics record` when the flag is omitted.
pub fn compute_forecast_hit_rate(conn: &Connection, window_days: i64) -> Result<Option<f64>> {
    let table_exists: i64 = conn
        .prepare(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'forecast_scores'",
        )?
        .query_row([], |row| row.get(0))
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(None);
    }
    let cutoff = (chrono::Utc::now().date_naive() - chrono::Duration::days(window_days))
        .format("%Y-%m-%d")
        .to_string();
    let (hits, total): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(direction_hit), 0), COUNT(*)
         FROM forecast_scores
         WHERE status = 'scored' AND direction_hit IS NOT NULL AND view_date >= ?1",
        params![cutoff],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if total == 0 {
        return Ok(None);
    }
    Ok(Some(
        (hits as f64 / total as f64 * 1000.0).round() / 1000.0,
    ))
}

// ---------------------------------------------------------------------------
// Conviction-price correlation (gold post-mortem T2, standing rule 15)
// ---------------------------------------------------------------------------

/// One (canonical layer × asset) conviction-vs-price correlation read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionPriceRow {
    pub layer: String,
    pub asset: String,
    /// Paired (conviction, close) observations found in the window.
    pub n: usize,
    /// Pearson r; None when n < CONVICTION_PRICE_MIN_N or either series has
    /// zero variance.
    pub r: Option<f64>,
    /// True when |r| > CONVICTION_PRICE_CORR_CEILING.
    pub flagged: bool,
    /// `ok` or `insufficient`.
    pub status: String,
}

/// Pearson correlation. None when fewer than 2 points or zero variance in
/// either series.
fn pearson(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() != ys.len() || xs.len() < 2 {
        return None;
    }
    let n = xs.len() as f64;
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;
    for (x, y) in xs.iter().zip(ys.iter()) {
        let dx = x - mean_x;
        let dy = y - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }
    if var_x == 0.0 || var_y == 0.0 {
        return None;
    }
    Some(cov / (var_x.sqrt() * var_y.sqrt()))
}

/// Per (canonical layer × asset): Pearson correlation between the layer's
/// signed conviction trajectory (`analyst_view_history`, latest view per day,
/// direction-authoritative signs — bear counts negative) and the asset's
/// `price_history` closes on matching dates over the trailing `days` window.
///
/// Closes are looked up under the asset symbol itself, falling back to the
/// `SYM-USD` twin. Pairs require an exact date match (a conviction written on
/// a day with no close is skipped). Needs `CONVICTION_PRICE_MIN_N` pairs for
/// a read; |r| > `CONVICTION_PRICE_CORR_CEILING` is flagged as momentum
/// dressed as structure (standing rule 15: conviction must not track price).
pub fn compute_conviction_price_correlations(
    conn: &Connection,
    assets: &[String],
    days: i64,
) -> Result<Vec<ConvictionPriceRow>> {
    use crate::db::analyst_views::CANONICAL_ANALYSTS;

    let table_exists: i64 = conn
        .prepare(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'analyst_view_history'",
        )?
        .query_row([], |row| row.get(0))
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(Vec::new());
    }
    let since = (chrono::Utc::now().date_naive() - chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();

    let mut out = Vec::new();
    for asset in assets {
        let asset_upper = asset.to_uppercase();
        // Resolve the price series once per asset: the symbol itself, else
        // its -USD twin.
        let series: Option<String> = {
            let has_rows = |sym: &str| -> bool {
                conn.prepare(
                    "SELECT COUNT(*) FROM price_history WHERE symbol = ?1 AND date >= ?2",
                )
                .and_then(|mut s| s.query_row(params![sym, since], |row| row.get::<_, i64>(0)))
                .unwrap_or(0)
                    > 0
            };
            if has_rows(&asset_upper) {
                Some(asset_upper.clone())
            } else {
                let twin = format!("{asset_upper}-USD");
                if !asset_upper.ends_with("-USD") && has_rows(&twin) {
                    Some(twin)
                } else {
                    None
                }
            }
        };

        for layer in CANONICAL_ANALYSTS {
            // Latest view per day (ascending scan, later rows win), signed
            // via direction-authoritative conviction.
            let mut stmt = conn.prepare(
                "SELECT date(recorded_at), direction, conviction
                 FROM analyst_view_history
                 WHERE analyst = ?1 AND upper(asset) = ?2 AND date(recorded_at) >= ?3
                 ORDER BY recorded_at ASC, id ASC",
            )?;
            let view_rows = stmt.query_map(params![layer, asset_upper, since], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?;
            let mut by_day: std::collections::BTreeMap<String, f64> =
                std::collections::BTreeMap::new();
            for r in view_rows {
                let (day, direction, conviction) = r?;
                by_day.insert(day, effective_conviction(&direction, conviction) as f64);
            }

            let mut convictions = Vec::new();
            let mut closes = Vec::new();
            if let Some(series) = &series {
                for (day, conviction) in &by_day {
                    let close: Option<String> = conn
                        .prepare(
                            "SELECT close FROM price_history WHERE symbol = ?1 AND date = ?2",
                        )?
                        .query_row(params![series, day], |row| row.get(0))
                        .ok();
                    if let Some(close) = close.and_then(|c| c.parse::<f64>().ok()) {
                        convictions.push(*conviction);
                        closes.push(close);
                    }
                }
            }

            let n = convictions.len();
            let r = if n >= CONVICTION_PRICE_MIN_N {
                pearson(&convictions, &closes)
            } else {
                None
            };
            let flagged = r.map(|v| v.abs() > CONVICTION_PRICE_CORR_CEILING) == Some(true);
            // Skip pairs with no data at all to keep the payload focused.
            if by_day.is_empty() {
                continue;
            }
            out.push(ConvictionPriceRow {
                layer: layer.to_string(),
                asset: asset_upper.clone(),
                n,
                r: r.map(|v| (v * 1000.0).round() / 1000.0),
                flagged,
                status: if r.is_some() { "ok" } else { "insufficient" }.to_string(),
            });
        }
    }
    Ok(out)
}

/// Max |r| across all computed (layer × asset) pairs — the value
/// `analytics epistemics record` self-derives into
/// `run_health.conviction_price_corr`. None when no pair has a read.
pub fn max_abs_conviction_price_corr(rows: &[ConvictionPriceRow]) -> Option<f64> {
    rows.iter()
        .filter_map(|row| row.r)
        .map(|r| r.abs())
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        })
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

    /// Minimal price_history shape for correlation tests (the in-memory
    /// schema from open_in_memory already has it, but keep tests explicit
    /// about the inserts they rely on).
    fn insert_close(conn: &Connection, symbol: &str, date: &str, close: &str) {
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source) VALUES (?1, ?2, ?3, 'test')
             ON CONFLICT(symbol, date) DO UPDATE SET close = excluded.close",
            params![symbol, date, close],
        )
        .unwrap();
    }

    fn insert_view(conn: &Connection, analyst: &str, asset: &str, date: &str, conviction: i64) {
        let direction = if conviction < 0 { "bear" } else { "bull" };
        conn.execute(
            "INSERT INTO analyst_view_history
                (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES (?1, ?2, ?3, ?4, 'r', ?5 || ' 12:00:00')",
            params![analyst, asset, direction, conviction.abs(), date],
        )
        .unwrap();
    }

    fn recent_dates(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| {
                (chrono::Utc::now().date_naive() - chrono::Duration::days((n - i) as i64))
                    .format("%Y-%m-%d")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn conviction_price_corr_flags_price_tracking_conviction() {
        let conn = setup();
        ensure_history_table(&conn);
        // Conviction rises in lockstep with price → r ≈ +1 → flagged.
        let dates = recent_dates(7);
        for (i, d) in dates.iter().enumerate() {
            insert_view(&conn, "medium", "GC=F", d, (i as i64) - 2); // -2..4
            insert_close(&conn, "GC=F", d, &format!("{}", 3000 + i * 50));
        }
        let rows =
            compute_conviction_price_correlations(&conn, &["GC=F".to_string()], 90).unwrap();
        let medium = rows
            .iter()
            .find(|r| r.layer == "medium" && r.asset == "GC=F")
            .expect("medium GC=F row");
        assert_eq!(medium.n, 7);
        let r = medium.r.expect("correlation read");
        assert!(r > 0.95, "lockstep trajectory must read r≈+1, got {r}");
        assert!(medium.flagged);
        assert_eq!(medium.status, "ok");
        assert_eq!(max_abs_conviction_price_corr(&rows), Some(r.abs()));
    }

    #[test]
    fn conviction_price_corr_clean_when_uncorrelated() {
        let conn = setup();
        ensure_history_table(&conn);
        // Alternating conviction against a monotonic price → r ≈ 0.
        let dates = recent_dates(8);
        for (i, d) in dates.iter().enumerate() {
            let conviction = if i % 2 == 0 { 3 } else { -3 };
            insert_view(&conn, "low", "BTC", d, conviction);
            insert_close(&conn, "BTC-USD", d, &format!("{}", 100000 + i * 10));
        }
        let rows = compute_conviction_price_correlations(&conn, &["BTC".to_string()], 90).unwrap();
        let low = rows
            .iter()
            .find(|r| r.layer == "low" && r.asset == "BTC")
            .expect("low BTC row (via -USD series fallback)");
        let r = low.r.expect("correlation read");
        assert!(r.abs() < 0.3, "alternating trajectory must read r≈0, got {r}");
        assert!(!low.flagged);
    }

    #[test]
    fn conviction_price_corr_insufficient_below_min_n() {
        let conn = setup();
        ensure_history_table(&conn);
        let dates = recent_dates(4); // below CONVICTION_PRICE_MIN_N
        for (i, d) in dates.iter().enumerate() {
            insert_view(&conn, "high", "SI=F", d, i as i64);
            insert_close(&conn, "SI=F", d, &format!("{}", 40 + i));
        }
        let rows = compute_conviction_price_correlations(&conn, &["SI=F".to_string()], 90).unwrap();
        let high = rows
            .iter()
            .find(|r| r.layer == "high" && r.asset == "SI=F")
            .expect("high SI=F row");
        assert_eq!(high.n, 4);
        assert!(high.r.is_none());
        assert_eq!(high.status, "insufficient");
        assert!(!high.flagged);
        assert_eq!(max_abs_conviction_price_corr(&rows), None);
    }

    #[test]
    fn conviction_price_corr_zero_variance_is_insufficient() {
        let conn = setup();
        ensure_history_table(&conn);
        // Constant conviction → zero variance → no read (and no flag),
        // even though n >= 6.
        let dates = recent_dates(7);
        for (i, d) in dates.iter().enumerate() {
            insert_view(&conn, "macro", "GC=F", d, 4);
            insert_close(&conn, "GC=F", d, &format!("{}", 3000 + i * 50));
        }
        let rows =
            compute_conviction_price_correlations(&conn, &["GC=F".to_string()], 90).unwrap();
        let row = rows
            .iter()
            .find(|r| r.layer == "macro" && r.asset == "GC=F")
            .unwrap();
        assert_eq!(row.n, 7);
        assert!(row.r.is_none());
        assert_eq!(row.status, "insufficient");
    }

    #[test]
    fn conviction_price_corr_merges_into_run_health_and_flags() {
        let conn = setup();
        upsert_run_health(
            &conn,
            "2026-06-10",
            &RunHealthInput {
                agreement_rate: Some(0.7),
                ..Default::default()
            },
        )
        .unwrap();
        // Field-wise merge: a later record call adds the correlation.
        upsert_run_health(
            &conn,
            "2026-06-10",
            &RunHealthInput {
                conviction_price_corr: Some(0.82),
                ..Default::default()
            },
        )
        .unwrap();
        let row = get_run_health(&conn, "2026-06-10").unwrap().unwrap();
        assert_eq!(row.agreement_rate, Some(0.7));
        assert_eq!(row.conviction_price_corr, Some(0.82));
        let flags = threshold_flags(&row);
        assert!(flags
            .iter()
            .any(|(m, w)| *m == "conviction_price_corr"
                && w.contains("momentum dressed as structure")
                && w.contains("standing rule 15")));

        // Below the ceiling → no flag.
        upsert_run_health(
            &conn,
            "2026-06-11",
            &RunHealthInput {
                conviction_price_corr: Some(0.4),
                ..Default::default()
            },
        )
        .unwrap();
        let clean = get_run_health(&conn, "2026-06-11").unwrap().unwrap();
        assert!(threshold_flags(&clean).is_empty());
    }

    #[test]
    fn forecast_fields_merge_and_misalignment_flag_fires() {
        let conn = setup();
        upsert_run_health(
            &conn,
            "2026-06-11",
            &RunHealthInput {
                agreement_rate: Some(0.7),
                ..Default::default()
            },
        )
        .unwrap();
        // Later record call adds the R2 fields (field-wise merge).
        upsert_run_health(
            &conn,
            "2026-06-11",
            &RunHealthInput {
                forecast_hit_rate: Some(0.41),
                active_misalignments: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        let row = get_run_health(&conn, "2026-06-11").unwrap().unwrap();
        assert_eq!(row.agreement_rate, Some(0.7));
        assert_eq!(row.forecast_hit_rate, Some(0.41));
        assert_eq!(row.active_misalignments, Some(2));
        let flags = threshold_flags(&row);
        assert!(flags
            .iter()
            .any(|(m, w)| *m == "active_misalignments"
                && w.contains("2 active forecast misalignment(s)")));

        // Zero misalignments → no flag.
        upsert_run_health(
            &conn,
            "2026-06-12",
            &RunHealthInput {
                active_misalignments: Some(0),
                forecast_hit_rate: Some(0.6),
                ..Default::default()
            },
        )
        .unwrap();
        let clean = get_run_health(&conn, "2026-06-12").unwrap().unwrap();
        assert!(threshold_flags(&clean).is_empty());
    }

    #[test]
    fn forecast_hit_rate_derives_over_trailing_window() {
        let conn = setup();
        crate::research::forecast_scoring::ensure_table(&conn).unwrap();
        // No scored rows yet → None.
        assert_eq!(compute_forecast_hit_rate(&conn, 30).unwrap(), None);

        let recent = (chrono::Utc::now().date_naive() - chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        let stale = (chrono::Utc::now().date_naive() - chrono::Duration::days(90))
            .format("%Y-%m-%d")
            .to_string();
        let insert = |id: i64, date: &str, hit: Option<bool>, status: &str| {
            conn.execute(
                "INSERT INTO forecast_scores
                    (view_history_id, analyst, asset, direction, conviction, horizon_days,
                     view_date, direction_hit, status)
                 VALUES (?1, 'medium', 'GC=F', 'bull', 3, 45, ?2, ?3, ?4)",
                params![id, date, hit.map(|h| h as i64), status],
            )
            .unwrap();
        };
        insert(1, &recent, Some(true), "scored");
        insert(2, &recent, Some(false), "scored");
        insert(3, &recent, Some(false), "scored");
        insert(4, &recent, None, "scored"); // neutral — excluded
        insert(5, &recent, None, "pending"); // unscored — excluded
        insert(6, &stale, Some(true), "scored"); // outside window — excluded

        let rate = compute_forecast_hit_rate(&conn, 30).unwrap().unwrap();
        assert!((rate - 1.0 / 3.0).abs() < 1e-3, "got {rate}");
        // Wider window picks up the stale hit: 2/4.
        let rate = compute_forecast_hit_rate(&conn, 120).unwrap().unwrap();
        assert!((rate - 0.5).abs() < 1e-9, "got {rate}");

        // Missing table degrades to None.
        let bare = Connection::open_in_memory().unwrap();
        assert_eq!(compute_forecast_hit_rate(&bare, 30).unwrap(), None);
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
