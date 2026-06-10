//! `recommendations` + `recommendation_outcomes` — the Recommendation → action
//! → outcome chain.
//!
//! Closes the loop between system-generated decision cards, the operator's
//! reply (journal/operator_replies), the resulting transaction (if any), and
//! the price action that follows. Together with the analytics CLI surface
//! (`pftui analytics recommendations ...`) this creates a rolling
//! recommendation-accuracy ledger.
//!
//! Schema notes:
//! - `recommendations.id` is referenced directly in markdown via
//!   `<!-- rec_id: N -->` so that downstream readers (the operator, agents,
//!   future enrichment) can resolve a card to a row without fuzzy matching.
//! - `recommendation_outcomes` uses `recommendation_id` as the PRIMARY KEY,
//!   guaranteeing one outcome row per recommendation.
//! - `outcome_score` is bounded to `[-100, 100]` and intentionally stored as
//!   `REAL` (not Decimal) — scores are dimensionless quality signals, not
//!   monetary values.

use anyhow::{anyhow, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Recommendation types accepted by the action-status pipeline. Aligned with
/// `operator_replies::VALID_DECISION_TYPES` so the auto-linker can match the
/// recommendation that produced a given reply.
pub const VALID_RECOMMENDATION_TYPES: &[&str] = &[
    "add",
    "wait",
    "trim",
    "hold",
    "exit",
    "avoid",
    "target-set",
    "target-remove",
    "target-ignore",
    "outlook-refine",
    "catalyst",
    "meta",
];

/// The recommendation-ledger action vocabulary (`pftui analytics
/// recommendations record`). A strict subset of
/// `VALID_RECOMMENDATION_TYPES`: the five actions that are mechanically
/// scoreable against forward returns. `wait` is a first-class recorded
/// action — for physically held assets (gold, silver) the system's job is
/// timing accumulation windows, and a correct WAIT through a drawdown is the
/// system doing its job.
pub const LEDGER_ACTIONS: &[&str] = &["add", "wait", "hold", "trim", "avoid"];

/// Forward-return scoring horizons (days, column name).
pub const FORWARD_HORIZONS: &[(i64, &str)] = &[
    (30, "fwd_30d_pct"),
    (90, "fwd_90d_pct"),
    (180, "fwd_180d_pct"),
];

pub const VALID_URGENCY: &[&str] = &["high", "normal", "low"];

pub const VALID_ACTION_STATUS: &[&str] = &[
    "accepted",
    "rejected",
    "partial",
    "deferred",
    "ignored",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recommendation {
    pub id: i64,
    pub report_date: String,
    pub asset: Option<String>,
    pub recommendation_type: String,
    pub urgency: String,
    pub rationale_summary: Option<String>,
    pub created_at: String,
    /// Decimal string: the close used to price the recommendation at record
    /// time (ledger rows only; legacy decision-card rows have NULL).
    pub entry_price: Option<String>,
    /// The price_history series that priced it (e.g. `GC=F` or `BTC-USD`).
    pub price_series: Option<String>,
    /// Which writer recorded it (default `decision-architect`).
    pub source: String,
    /// Scored forward returns (percent, vs entry_price). NULL until the
    /// horizon elapses and `score` fills it; never overwritten once set.
    pub fwd_30d_pct: Option<f64>,
    pub fwd_90d_pct: Option<f64>,
    pub fwd_180d_pct: Option<f64>,
    /// Timestamp of the most recent forward-return scoring pass that filled
    /// at least one horizon on this row.
    pub scored_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RecommendationInsert<'a> {
    pub report_date: &'a str,
    pub asset: Option<&'a str>,
    pub recommendation_type: &'a str,
    pub urgency: &'a str,
    pub rationale_summary: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecommendationOutcome {
    pub recommendation_id: i64,
    pub operator_reply_id: Option<i64>,
    pub action_status: Option<String>,
    pub transaction_id: Option<i64>,
    pub outcome_score: Option<f64>,
    pub outcome_evaluated_at: Option<String>,
    pub outcome_notes: Option<String>,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS recommendations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            report_date TEXT NOT NULL,
            asset TEXT,
            recommendation_type TEXT NOT NULL,
            urgency TEXT NOT NULL,
            rationale_summary TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            entry_price TEXT,
            price_series TEXT,
            source TEXT NOT NULL DEFAULT 'decision-architect',
            fwd_30d_pct REAL,
            fwd_90d_pct REAL,
            fwd_180d_pct REAL,
            scored_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_recommendations_report_date
            ON recommendations(report_date);
        CREATE INDEX IF NOT EXISTS idx_recommendations_asset
            ON recommendations(asset);
        CREATE INDEX IF NOT EXISTS idx_recommendations_type
            ON recommendations(recommendation_type);

        CREATE TABLE IF NOT EXISTS recommendation_outcomes (
            recommendation_id INTEGER PRIMARY KEY REFERENCES recommendations(id) ON DELETE CASCADE,
            operator_reply_id INTEGER REFERENCES operator_replies(id),
            action_status TEXT CHECK(action_status IN (
                'accepted','rejected','partial','deferred','ignored'
            )),
            transaction_id INTEGER REFERENCES transactions(id),
            outcome_score REAL CHECK(outcome_score BETWEEN -100 AND 100),
            outcome_evaluated_at TEXT,
            outcome_notes TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_rec_outcomes_reply
            ON recommendation_outcomes(operator_reply_id);
        CREATE INDEX IF NOT EXISTS idx_rec_outcomes_tx
            ON recommendation_outcomes(transaction_id);",
    )?;

    // Migration (gold post-mortem T2): recommendation-ledger columns.
    // CREATE TABLE IF NOT EXISTS never adds columns to an existing table, so
    // DBs created before the ledger shipped self-heal here. Additive and
    // idempotent via pragma_table_info, mirroring the calibration_matrix
    // pattern in db/schema.rs. Order matches the canonical CREATE above so a
    // migrated table and a fresh table have identical column order.
    for (column, ddl) in &[
        (
            "entry_price",
            "ALTER TABLE recommendations ADD COLUMN entry_price TEXT",
        ),
        (
            "price_series",
            "ALTER TABLE recommendations ADD COLUMN price_series TEXT",
        ),
        (
            "source",
            "ALTER TABLE recommendations ADD COLUMN source TEXT NOT NULL DEFAULT 'decision-architect'",
        ),
        (
            "fwd_30d_pct",
            "ALTER TABLE recommendations ADD COLUMN fwd_30d_pct REAL",
        ),
        (
            "fwd_90d_pct",
            "ALTER TABLE recommendations ADD COLUMN fwd_90d_pct REAL",
        ),
        (
            "fwd_180d_pct",
            "ALTER TABLE recommendations ADD COLUMN fwd_180d_pct REAL",
        ),
        (
            "scored_at",
            "ALTER TABLE recommendations ADD COLUMN scored_at TEXT",
        ),
    ] {
        let exists: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('recommendations') WHERE name = ?1")?
            .query_row([column], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }
    Ok(())
}

/// Canonical SELECT column list for `Recommendation` rows. `prefix` lets
/// callers qualify with a table alias (pass `"r."` in joins, `""` otherwise).
fn rec_columns(prefix: &str) -> String {
    [
        "id",
        "report_date",
        "asset",
        "recommendation_type",
        "urgency",
        "rationale_summary",
        "created_at",
        "entry_price",
        "price_series",
        "source",
        "fwd_30d_pct",
        "fwd_90d_pct",
        "fwd_180d_pct",
        "scored_at",
    ]
    .iter()
    .map(|c| format!("{prefix}{c}"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn row_to_recommendation(row: &rusqlite::Row) -> Result<Recommendation, rusqlite::Error> {
    Ok(Recommendation {
        id: row.get(0)?,
        report_date: row.get(1)?,
        asset: row.get(2)?,
        recommendation_type: row.get(3)?,
        urgency: row.get(4)?,
        rationale_summary: row.get(5)?,
        created_at: row.get(6)?,
        entry_price: row.get(7)?,
        price_series: row.get(8)?,
        source: row.get(9)?,
        fwd_30d_pct: row.get(10)?,
        fwd_90d_pct: row.get(11)?,
        fwd_180d_pct: row.get(12)?,
        scored_at: row.get(13)?,
    })
}

/// Insert a new recommendation row. Returns the newly assigned id.
pub fn insert_recommendation(conn: &Connection, row: &RecommendationInsert<'_>) -> Result<i64> {
    ensure_table(conn)?;
    if !VALID_RECOMMENDATION_TYPES.contains(&row.recommendation_type) {
        return Err(anyhow!(
            "invalid recommendation_type '{}'; must be one of {}",
            row.recommendation_type,
            VALID_RECOMMENDATION_TYPES.join("|")
        ));
    }
    if !VALID_URGENCY.contains(&row.urgency) {
        return Err(anyhow!(
            "invalid urgency '{}'; must be one of {}",
            row.urgency,
            VALID_URGENCY.join("|")
        ));
    }
    conn.execute(
        "INSERT INTO recommendations
            (report_date, asset, recommendation_type, urgency, rationale_summary)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            row.report_date,
            row.asset,
            row.recommendation_type,
            row.urgency,
            row.rationale_summary,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Upsert a recommendation deduplicated on `(report_date, asset, recommendation_type)`.
/// If a matching row exists, returns its id without modification. Otherwise
/// inserts a fresh row.
pub fn upsert_recommendation(conn: &Connection, row: &RecommendationInsert<'_>) -> Result<i64> {
    ensure_table(conn)?;
    let existing: Option<i64> = conn
        .prepare(
            "SELECT id FROM recommendations
             WHERE report_date = ?1
               AND COALESCE(asset, '') = COALESCE(?2, '')
               AND recommendation_type = ?3
             LIMIT 1",
        )?
        .query_row(
            params![row.report_date, row.asset, row.recommendation_type],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }
    insert_recommendation(conn, row)
}

pub fn list(
    conn: &Connection,
    report_date: Option<&str>,
    asset: Option<&str>,
    recommendation_type: Option<&str>,
    since: Option<&str>,
) -> Result<Vec<Recommendation>> {
    ensure_table(conn)?;
    let mut sql = format!("SELECT {} FROM recommendations WHERE 1=1", rec_columns(""));
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(d) = report_date {
        sql.push_str(" AND report_date = ?");
        args.push(Box::new(d.to_string()));
    }
    if let Some(a) = asset {
        sql.push_str(" AND asset = ?");
        args.push(Box::new(a.to_string()));
    }
    if let Some(t) = recommendation_type {
        sql.push_str(" AND recommendation_type = ?");
        args.push(Box::new(t.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND report_date >= ?");
        args.push(Box::new(s.to_string()));
    }
    sql.push_str(" ORDER BY report_date DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), row_to_recommendation)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Recommendation>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM recommendations WHERE id = ?1",
        rec_columns("")
    ))?;
    let row = stmt.query_row(params![id], row_to_recommendation).ok();
    Ok(row)
}

pub fn get_outcome(conn: &Connection, id: i64) -> Result<Option<RecommendationOutcome>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT recommendation_id, operator_reply_id, action_status,
                transaction_id, outcome_score, outcome_evaluated_at, outcome_notes
         FROM recommendation_outcomes WHERE recommendation_id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |row| {
            Ok(RecommendationOutcome {
                recommendation_id: row.get(0)?,
                operator_reply_id: row.get(1)?,
                action_status: row.get(2)?,
                transaction_id: row.get(3)?,
                outcome_score: row.get(4)?,
                outcome_evaluated_at: row.get(5)?,
                outcome_notes: row.get(6)?,
            })
        })
        .ok();
    Ok(row)
}

/// Set the operator_reply linkage for a recommendation. Creates the outcome row
/// if necessary. If `action_status` is provided, also writes it; otherwise the
/// existing status is preserved.
pub fn link_operator_reply(
    conn: &Connection,
    recommendation_id: i64,
    operator_reply_id: i64,
    action_status: Option<&str>,
) -> Result<()> {
    ensure_table(conn)?;
    if let Some(status) = action_status {
        if !VALID_ACTION_STATUS.contains(&status) {
            return Err(anyhow!(
                "invalid action_status '{}'; must be one of {}",
                status,
                VALID_ACTION_STATUS.join("|")
            ));
        }
    }
    conn.execute(
        "INSERT INTO recommendation_outcomes
            (recommendation_id, operator_reply_id, action_status)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(recommendation_id) DO UPDATE SET
            operator_reply_id = excluded.operator_reply_id,
            action_status = COALESCE(excluded.action_status, recommendation_outcomes.action_status)",
        params![recommendation_id, operator_reply_id, action_status],
    )?;
    Ok(())
}

/// Set the transaction linkage for a recommendation. Creates the outcome row
/// if necessary. If `action_status` is not yet recorded, it is defaulted to
/// `accepted` on first link.
pub fn link_transaction(
    conn: &Connection,
    recommendation_id: i64,
    transaction_id: i64,
) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO recommendation_outcomes
            (recommendation_id, transaction_id, action_status)
         VALUES (?1, ?2, 'accepted')
         ON CONFLICT(recommendation_id) DO UPDATE SET
            transaction_id = excluded.transaction_id,
            action_status = COALESCE(recommendation_outcomes.action_status, 'accepted')",
        params![recommendation_id, transaction_id],
    )?;
    Ok(())
}

/// Persist the computed outcome score for a recommendation. Replaces any
/// existing score and timestamps the evaluation.
pub fn set_outcome_score(
    conn: &Connection,
    recommendation_id: i64,
    outcome_score: f64,
    evaluated_at: &str,
    outcome_notes: Option<&str>,
) -> Result<()> {
    ensure_table(conn)?;
    if !(-100.0..=100.0).contains(&outcome_score) {
        return Err(anyhow!(
            "outcome_score {outcome_score} out of bounds (-100, 100)"
        ));
    }
    conn.execute(
        "INSERT INTO recommendation_outcomes
            (recommendation_id, outcome_score, outcome_evaluated_at, outcome_notes)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(recommendation_id) DO UPDATE SET
            outcome_score = excluded.outcome_score,
            outcome_evaluated_at = excluded.outcome_evaluated_at,
            outcome_notes = COALESCE(excluded.outcome_notes, recommendation_outcomes.outcome_notes)",
        params![recommendation_id, outcome_score, evaluated_at, outcome_notes],
    )?;
    Ok(())
}

/// Compute an outcome score from price action.
///
/// Score formula (in `[-100, 100]`):
/// - `add` (or `target-set` with bullish bias): linear with %-price change,
///   clipped to ±100 at ±20%.
/// - `trim`/`exit`: linear with NEGATIVE %-price change (a fall after a trim
///   is a positive outcome), clipped to ±100 at ±20%.
/// - `hold`: rewards small absolute moves (within ±5% gets +100, decays to
///   0 at ±20%, then negative beyond).
/// - other types: returns 0.0 as a neutral score.
///
/// Returns `None` when either price is missing.
pub fn compute_outcome_score(
    recommendation_type: &str,
    start_price: Option<Decimal>,
    end_price: Option<Decimal>,
) -> Option<f64> {
    let (start, end) = match (start_price, end_price) {
        (Some(s), Some(e)) if s != Decimal::ZERO => (s, e),
        _ => return None,
    };
    // Compute pct change as f64 — score is dimensionless and bounded.
    let change_pct = ((end - start) / start * Decimal::from(100))
        .to_string()
        .parse::<f64>()
        .ok()?;

    let clip = |v: f64| v.clamp(-100.0, 100.0);
    let scale = |pct: f64| pct * 5.0; // ±20% maps to ±100

    let score = match recommendation_type {
        "add" | "target-set" => clip(scale(change_pct)),
        "trim" | "exit" | "target-remove" => clip(-scale(change_pct)),
        "hold" | "target-ignore" => {
            let abs = change_pct.abs();
            if abs <= 5.0 {
                100.0
            } else if abs <= 20.0 {
                // Linear from +100 at 5% to -100 at 20% (range 15pp).
                let frac = (abs - 5.0) / 15.0; // 0..1
                100.0 - 200.0 * frac
            } else {
                -100.0
            }
        }
        _ => 0.0,
    };
    Some(score)
}

/// Pull the closest price on/before `date` for `asset`.
pub fn price_at(conn: &Connection, asset: &str, date: &str) -> Result<Option<Decimal>> {
    crate::db::price_history::get_price_at_date(conn, asset, date)
}

/// Aggregate accuracy summary over a window. Hit rate is the share of scored
/// outcomes with `outcome_score >= threshold`.
#[derive(Debug, Clone, Serialize)]
pub struct AccuracyBucket {
    pub recommendation_type: String,
    pub asset: Option<String>,
    pub scored: u32,
    pub total: u32,
    pub hits: u32,
    pub hit_rate_pct: f64,
    pub avg_score: f64,
}

/// Compute per-type accuracy. If `group_by_asset` is true, the bucket key
/// includes asset and the result is keyed `(type, asset)`.
pub fn accuracy_summary(
    conn: &Connection,
    recommendation_type: Option<&str>,
    asset: Option<&str>,
    since: Option<&str>,
    threshold: f64,
    group_by_asset: bool,
) -> Result<Vec<AccuracyBucket>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT r.recommendation_type, r.asset, o.outcome_score
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(t) = recommendation_type {
        sql.push_str(" AND r.recommendation_type = ?");
        args.push(Box::new(t.to_string()));
    }
    if let Some(a) = asset {
        sql.push_str(" AND r.asset = ?");
        args.push(Box::new(a.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND r.report_date >= ?");
        args.push(Box::new(s.to_string()));
    }
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows: Vec<(String, Option<String>, Option<f64>)> = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<f64>>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    use std::collections::BTreeMap;
    // (total, scored, hits, score_sum) per (recommendation_type, asset?) key.
    type BucketAgg = (u32, u32, u32, f64);
    let mut buckets: BTreeMap<(String, Option<String>), BucketAgg> = BTreeMap::new();
    for (rtype, ras, score) in rows {
        let key = if group_by_asset {
            (rtype, ras)
        } else {
            (rtype, None)
        };
        let entry = buckets.entry(key).or_insert((0, 0, 0, 0.0));
        entry.0 += 1; // total
        if let Some(s) = score {
            entry.1 += 1; // scored
            entry.3 += s;
            if s >= threshold {
                entry.2 += 1; // hits
            }
        }
    }
    let result = buckets
        .into_iter()
        .map(|((rtype, ras), (total, scored, hits, sum))| {
            let hit_rate = if scored > 0 {
                hits as f64 / scored as f64 * 100.0
            } else {
                0.0
            };
            let avg = if scored > 0 { sum / scored as f64 } else { 0.0 };
            AccuracyBucket {
                recommendation_type: rtype,
                asset: ras,
                scored,
                total,
                hits,
                hit_rate_pct: hit_rate,
                avg_score: avg,
            }
        })
        .collect::<Vec<_>>();
    Ok(result)
}

/// Find an open recommendation matching the given report_date and asset. Used
/// by the operator_reply auto-linker.
pub fn find_open_for_reply(
    conn: &Connection,
    report_date: &str,
    asset: Option<&str>,
    decision_type: Option<&str>,
) -> Result<Option<Recommendation>> {
    ensure_table(conn)?;
    let mut sql = format!(
        "SELECT {}
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE r.report_date = ?1
           AND o.operator_reply_id IS NULL",
        rec_columns("r.")
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(report_date.to_string())];
    if let Some(a) = asset {
        sql.push_str(" AND r.asset = ?");
        args.push(Box::new(a.to_string()));
    }
    if let Some(t) = decision_type {
        sql.push_str(" AND r.recommendation_type = ?");
        args.push(Box::new(t.to_string()));
    }
    sql.push_str(" ORDER BY r.id DESC LIMIT 1");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let row = stmt
        .query_row(params_slice.as_slice(), row_to_recommendation)
        .ok();
    Ok(row)
}

/// Find an open recommendation for `(asset, tx_direction)` whose `report_date`
/// falls within `window_days` BEFORE `tx_date`. Used by the transaction
/// auto-linker.
///
/// `tx_direction` is the transaction direction (e.g. `buy` → matches `add`;
/// `sell` → matches `trim`/`exit`).
pub fn find_open_for_transaction(
    conn: &Connection,
    asset: &str,
    tx_direction: &str,
    tx_date: &str,
    window_days: i64,
) -> Result<Option<Recommendation>> {
    ensure_table(conn)?;
    let matching_types: &[&str] = match tx_direction.to_lowercase().as_str() {
        "buy" => &["add", "target-set"],
        "sell" => &["trim", "exit", "target-remove"],
        _ => return Ok(None),
    };
    let types_placeholder = matching_types
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 4))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT {}
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE r.asset = ?1
           AND r.recommendation_type IN ({})
           AND r.report_date <= ?2
           AND julianday(?2) - julianday(r.report_date) <= ?3
           AND (o.transaction_id IS NULL)
         ORDER BY r.report_date DESC LIMIT 1",
        rec_columns("r."),
        types_placeholder
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(asset.to_string()),
        Box::new(tx_date.to_string()),
        Box::new(window_days as f64),
    ];
    for t in matching_types {
        args.push(Box::new(t.to_string()));
    }
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let row = stmt
        .query_row(params_slice.as_slice(), row_to_recommendation)
        .ok();
    Ok(row)
}

/// List all recommendations that do not yet have a stored outcome score.
/// Useful for `pftui analytics recommendations score --all`.
pub fn list_unscored(conn: &Connection, since: Option<&str>) -> Result<Vec<Recommendation>> {
    ensure_table(conn)?;
    let mut sql = format!(
        "SELECT {}
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE (o.outcome_score IS NULL)",
        rec_columns("r.")
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = since {
        sql.push_str(" AND r.report_date >= ?");
        args.push(Box::new(s.to_string()));
    }
    sql.push_str(" ORDER BY r.report_date ASC, r.id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), row_to_recommendation)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Compute the rolling 7-day recommendation hit rate. Looks at recommendations
/// from `(today - window_days)` to today; if no scored outcomes, returns None.
/// Hit threshold defaults to 0.0 (positive score) when caller passes 0.0.
pub fn rolling_hit_rate(
    conn: &Connection,
    today: &str,
    window_days: i64,
    threshold: f64,
) -> Result<Option<RollingHitRate>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT o.outcome_score
         FROM recommendations r
         JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE julianday(?1) - julianday(r.report_date) BETWEEN 0 AND ?2
           AND o.outcome_score IS NOT NULL",
    )?;
    let scores: Vec<f64> = stmt
        .query_map(params![today, window_days as f64], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if scores.is_empty() {
        return Ok(None);
    }
    let scored = scores.len() as u32;
    let hits = scores.iter().filter(|s| **s >= threshold).count() as u32;
    let avg = scores.iter().sum::<f64>() / scored as f64;
    Ok(Some(RollingHitRate {
        window_days,
        scored,
        hits,
        hit_rate_pct: hits as f64 / scored as f64 * 100.0,
        avg_score: avg,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct RollingHitRate {
    pub window_days: i64,
    pub scored: u32,
    pub hits: u32,
    pub hit_rate_pct: f64,
    pub avg_score: f64,
}

// ---------------------------------------------------------------------------
// Recommendation ledger (gold post-mortem T2)
//
// A 5-month post-mortem found the system recommended adding gold every single
// run into a -19% drawdown and could not notice, because recommendations —
// unlike predictions — were never scored. The ledger fixes that: every
// decision-card action is recorded with the close that priced it, forward
// returns are filled mechanically when each horizon elapses, and the
// scoreboard makes the timing quality of ADD vs WAIT calls measurable.
// ---------------------------------------------------------------------------

/// Resolve the price series that can price `symbol` at `date`: the symbol
/// itself, else the `SYMBOL-USD` twin (crypto held under its bare ticker).
/// Returns `(series, close)` or `None` when neither has history.
pub fn resolve_entry_price(
    conn: &Connection,
    symbol: &str,
    date: &str,
) -> Result<Option<(String, Decimal)>> {
    if let Some(close) = crate::db::price_history::get_price_at_date(conn, symbol, date)? {
        if close != Decimal::ZERO {
            return Ok(Some((symbol.to_string(), close)));
        }
    }
    if !symbol.to_uppercase().ends_with("-USD") {
        let twin = format!("{symbol}-USD");
        if let Some(close) = crate::db::price_history::get_price_at_date(conn, &twin, date)? {
            if close != Decimal::ZERO {
                return Ok(Some((twin, close)));
            }
        }
    }
    Ok(None)
}

/// Record one ledger entry. `action` must be one of `LEDGER_ACTIONS`.
/// `entry_price` is auto-filled from the latest `price_history` close on or
/// before `run_date` (falling back `SYM` → `SYM-USD`; the series that priced
/// it is stored in `price_series`). A missing price is not an error — the row
/// is recorded unpriced and simply never accrues forward returns.
pub fn record_ledger_entry(
    conn: &Connection,
    run_date: &str,
    symbol: &str,
    action: &str,
    rationale: Option<&str>,
    source: &str,
) -> Result<Recommendation> {
    ensure_table(conn)?;
    if !LEDGER_ACTIONS.contains(&action) {
        return Err(anyhow!(
            "invalid action '{}'; must be one of {}",
            action,
            LEDGER_ACTIONS.join("|")
        ));
    }
    let symbol = symbol.to_uppercase();
    let priced = resolve_entry_price(conn, &symbol, run_date)?;
    let (price_series, entry_price) = match &priced {
        Some((series, close)) => (Some(series.clone()), Some(close.to_string())),
        None => (None, None),
    };
    conn.execute(
        "INSERT INTO recommendations
            (report_date, asset, recommendation_type, urgency, rationale_summary,
             entry_price, price_series, source)
         VALUES (?1, ?2, ?3, 'normal', ?4, ?5, ?6, ?7)",
        params![run_date, symbol, action, rationale, entry_price, price_series, source],
    )?;
    let id = conn.last_insert_rowid();
    get(conn, id)?.ok_or_else(|| anyhow!("recommendation row vanished after insert"))
}

/// Summary of one forward-return scoring pass.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ForwardScoreSummary {
    /// Rows examined (priced rows with at least one unscored horizon).
    pub candidates: usize,
    /// Individual (row, horizon) cells filled this pass.
    pub horizons_filled: usize,
    /// Distinct rows that received at least one new horizon.
    pub rows_updated: usize,
}

/// Fill `fwd_{30,90,180}d_pct` for any row whose horizon has elapsed:
/// percent change from `entry_price` to the close at `run_date + N` (closest
/// close on or before that date, but strictly after `run_date` so a data gap
/// can't score a row against its own entry close). Idempotent — a scored
/// horizon is never overwritten, and unscorable cells are retried on the
/// next pass once price history arrives.
pub fn score_forward_returns(conn: &Connection, today: &str) -> Result<ForwardScoreSummary> {
    ensure_table(conn)?;
    let today_date = NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map_err(|_| anyhow!("invalid date '{}': expected YYYY-MM-DD", today))?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM recommendations
         WHERE entry_price IS NOT NULL
           AND (fwd_30d_pct IS NULL OR fwd_90d_pct IS NULL OR fwd_180d_pct IS NULL)
         ORDER BY report_date ASC, id ASC",
        rec_columns("")
    ))?;
    let rows = stmt
        .query_map([], row_to_recommendation)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut summary = ForwardScoreSummary {
        candidates: rows.len(),
        ..Default::default()
    };
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    for rec in &rows {
        let Ok(run_date) = NaiveDate::parse_from_str(&rec.report_date, "%Y-%m-%d") else {
            continue;
        };
        let Some(entry) = rec.entry_price.as_deref().and_then(|p| Decimal::from_str(p).ok())
        else {
            continue;
        };
        if entry == Decimal::ZERO {
            continue;
        }
        let series = rec
            .price_series
            .clone()
            .or_else(|| rec.asset.clone())
            .unwrap_or_default();
        if series.is_empty() {
            continue;
        }
        let mut filled_any = false;
        for (days, column) in FORWARD_HORIZONS {
            let already = match *column {
                "fwd_30d_pct" => rec.fwd_30d_pct.is_some(),
                "fwd_90d_pct" => rec.fwd_90d_pct.is_some(),
                _ => rec.fwd_180d_pct.is_some(),
            };
            if already {
                continue;
            }
            let horizon_date = run_date + chrono::Duration::days(*days);
            if horizon_date > today_date {
                continue; // horizon not elapsed yet
            }
            let horizon_str = horizon_date.format("%Y-%m-%d").to_string();
            // Closest close on or before run_date+N, strictly after run_date.
            let close: Option<String> = conn
                .prepare(
                    "SELECT close FROM price_history
                     WHERE symbol = ?1 AND date <= ?2 AND date > ?3
                     ORDER BY date DESC LIMIT 1",
                )?
                .query_row(params![series, horizon_str, rec.report_date], |row| {
                    row.get(0)
                })
                .ok();
            let Some(close) = close.and_then(|c| Decimal::from_str(&c).ok()) else {
                continue;
            };
            let pct = ((close - entry) / entry * Decimal::from(100))
                .to_string()
                .parse::<f64>()
                .unwrap_or(0.0);
            // COALESCE guard: never overwrite a scored horizon, even if a
            // concurrent pass beat us to it.
            conn.execute(
                &format!(
                    "UPDATE recommendations
                     SET {column} = COALESCE({column}, ?1), scored_at = ?2
                     WHERE id = ?3"
                ),
                params![pct, now, rec.id],
            )?;
            summary.horizons_filled += 1;
            filled_any = true;
        }
        if filled_any {
            summary.rows_updated += 1;
        }
    }
    Ok(summary)
}

/// Per-horizon aggregate for one (symbol, action) scoreboard cell.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScoreboardCell {
    pub n: usize,
    pub positive: usize,
    pub pct_positive: f64,
    pub mean_pct: f64,
}

fn cell_from(values: &[f64]) -> Option<ScoreboardCell> {
    if values.is_empty() {
        return None;
    }
    let n = values.len();
    let positive = values.iter().filter(|v| **v > 0.0).count();
    let mean = values.iter().sum::<f64>() / n as f64;
    Some(ScoreboardCell {
        n,
        positive,
        pct_positive: positive as f64 / n as f64 * 100.0,
        mean_pct: mean,
    })
}

/// One scoreboard row: symbol × action with per-horizon stats.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreboardRow {
    pub symbol: String,
    pub action: String,
    /// All recorded ledger rows for this (symbol, action), scored or not.
    pub n_total: usize,
    pub h30: Option<ScoreboardCell>,
    pub h90: Option<ScoreboardCell>,
    pub h180: Option<ScoreboardCell>,
}

/// The WINDOW-QUALITY metric per symbol: mean 90d forward return after ADD
/// minus after WAIT. Positive → the system's ADD timing added value over
/// just waiting. Negative → its ADD calls were worse than its WAIT calls
/// (the gold failure, made measurable).
#[derive(Debug, Clone, Serialize)]
pub struct WindowQuality {
    pub symbol: String,
    pub add_n: usize,
    pub wait_n: usize,
    pub add_mean_90d_pct: Option<f64>,
    pub wait_mean_90d_pct: Option<f64>,
    /// `add_mean_90d_pct - wait_mean_90d_pct`; None until both sides have a
    /// scored 90d return.
    pub delta_pct: Option<f64>,
}

/// Full scoreboard payload.
#[derive(Debug, Clone, Serialize)]
pub struct Scoreboard {
    pub rows: Vec<ScoreboardRow>,
    pub window_quality: Vec<WindowQuality>,
    /// Ledger rows with no scored horizon yet (still accruing).
    pub unscored: usize,
}

/// Build the scoreboard over ledger-action rows (optionally one symbol).
pub fn scoreboard(conn: &Connection, symbol: Option<&str>) -> Result<Scoreboard> {
    ensure_table(conn)?;
    let actions_in = LEDGER_ACTIONS
        .iter()
        .map(|a| format!("'{a}'"))
        .collect::<Vec<_>>()
        .join(",");
    let mut sql = format!(
        "SELECT asset, recommendation_type, fwd_30d_pct, fwd_90d_pct, fwd_180d_pct
         FROM recommendations
         WHERE asset IS NOT NULL AND recommendation_type IN ({actions_in})"
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = symbol {
        sql.push_str(" AND upper(asset) = upper(?)");
        args.push(Box::new(s.to_string()));
    }
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    /// (asset, action, fwd_30d_pct, fwd_90d_pct, fwd_180d_pct)
    type LedgerCells = (String, String, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<LedgerCells> = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Agg {
        n_total: usize,
        h30: Vec<f64>,
        h90: Vec<f64>,
        h180: Vec<f64>,
    }
    let mut by_key: BTreeMap<(String, String), Agg> = BTreeMap::new();
    let mut unscored = 0usize;
    for (asset, action, f30, f90, f180) in rows {
        let entry = by_key.entry((asset.to_uppercase(), action)).or_default();
        entry.n_total += 1;
        if f30.is_none() && f90.is_none() && f180.is_none() {
            unscored += 1;
        }
        if let Some(v) = f30 {
            entry.h30.push(v);
        }
        if let Some(v) = f90 {
            entry.h90.push(v);
        }
        if let Some(v) = f180 {
            entry.h180.push(v);
        }
    }

    let mut out_rows = Vec::new();
    let mut per_symbol_90: BTreeMap<String, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for ((sym, action), agg) in by_key {
        match action.as_str() {
            "add" => per_symbol_90
                .entry(sym.clone())
                .or_default()
                .0
                .extend(agg.h90.iter().copied()),
            "wait" => per_symbol_90
                .entry(sym.clone())
                .or_default()
                .1
                .extend(agg.h90.iter().copied()),
            _ => {}
        }
        out_rows.push(ScoreboardRow {
            symbol: sym,
            action,
            n_total: agg.n_total,
            h30: cell_from(&agg.h30),
            h90: cell_from(&agg.h90),
            h180: cell_from(&agg.h180),
        });
    }

    let window_quality = per_symbol_90
        .into_iter()
        .filter(|(_, (adds, waits))| !adds.is_empty() || !waits.is_empty())
        .map(|(sym, (adds, waits))| {
            let add_mean = cell_from(&adds).map(|c| c.mean_pct);
            let wait_mean = cell_from(&waits).map(|c| c.mean_pct);
            let delta = match (add_mean, wait_mean) {
                (Some(a), Some(w)) => Some(a - w),
                _ => None,
            };
            WindowQuality {
                symbol: sym,
                add_n: adds.len(),
                wait_n: waits.len(),
                add_mean_90d_pct: add_mean,
                wait_mean_90d_pct: wait_mean,
                delta_pct: delta,
            }
        })
        .collect();

    Ok(Scoreboard {
        rows: out_rows,
        window_quality,
        unscored,
    })
}

/// Helper to detect a DECISION REPLY style payload in a free-text journal
/// entry. Returns the matched decision attributes if the content begins with
/// `DECISION REPLY` (case-insensitive) and includes a structured payload.
///
/// Recognised payload form (KEY=VALUE pairs, space- or comma-separated):
///
///   DECISION REPLY asset=BTC type=add response=yes [report_date=YYYY-MM-DD]
///
/// `asset` and `type` are required; `response` defaults to `yes`. The match is
/// intentionally permissive so the operator can dictate inline.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionReplyMatch {
    pub asset: String,
    pub decision_type: String,
    pub response_class: String,
    pub report_date: Option<String>,
    pub reasoning_summary: Option<String>,
}

pub fn parse_decision_reply(content: &str) -> Option<DecisionReplyMatch> {
    let trimmed = content.trim_start();
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("decision reply") && !lower.starts_with("decision-reply") {
        return None;
    }
    let mut asset: Option<String> = None;
    let mut decision_type: Option<String> = None;
    let mut response_class = "yes".to_string();
    let mut report_date: Option<String> = None;
    let mut reasoning: Option<String> = None;

    let payload = trimmed
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .unwrap_or("");

    for chunk in payload
        .split([',', ';', '\n'])
        .flat_map(|seg| seg.split_whitespace())
    {
        if let Some((k, v)) = chunk.split_once('=') {
            let k = k.trim().to_lowercase();
            let v = v.trim().trim_matches('"').to_string();
            if v.is_empty() {
                continue;
            }
            match k.as_str() {
                "asset" | "symbol" => asset = Some(v.to_uppercase()),
                "type" | "decision" => decision_type = Some(v.to_lowercase()),
                "response" | "class" => response_class = v.to_lowercase(),
                "report_date" | "date" => report_date = Some(v),
                "reason" | "reasoning" => reasoning = Some(v),
                _ => {}
            }
        }
    }

    Some(DecisionReplyMatch {
        asset: asset?,
        decision_type: decision_type?,
        response_class,
        report_date,
        reasoning_summary: reasoning,
    })
}

/// Convenience: parse a number string into a `Decimal` (never panics).
#[allow(dead_code)]
pub fn parse_decimal(s: &str) -> Option<Decimal> {
    Decimal::from_str(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::operator_replies;
    use rust_decimal_macros::dec;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Required parents for FK references.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                PRIMARY KEY (symbol, date)
            );",
        )
        .unwrap();
        operator_replies::ensure_table(&conn).unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_and_list_roundtrips() {
        let conn = fresh_conn();
        let id = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "high",
                rationale_summary: Some("convergent bull"),
            },
        )
        .unwrap();
        assert!(id > 0);
        let rows = list(&conn, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset.as_deref(), Some("BTC"));
    }

    #[test]
    fn upsert_dedupes_on_date_asset_type() {
        let conn = fresh_conn();
        let row = RecommendationInsert {
            report_date: "2026-05-28",
            asset: Some("BTC"),
            recommendation_type: "add",
            urgency: "high",
            rationale_summary: Some("first"),
        };
        let a = upsert_recommendation(&conn, &row).unwrap();
        let b = upsert_recommendation(&conn, &row).unwrap();
        assert_eq!(a, b);
        assert_eq!(list(&conn, None, None, None, None).unwrap().len(), 1);
    }

    #[test]
    fn invalid_type_or_urgency_rejected() {
        let conn = fresh_conn();
        let bad = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "smash",
                urgency: "high",
                rationale_summary: None,
            },
        );
        assert!(bad.is_err());
    }

    #[test]
    fn compute_outcome_score_add_rewards_up_moves() {
        let s = compute_outcome_score("add", Some(dec!(100)), Some(dec!(110))).unwrap();
        // +10% * 5 = +50.
        assert!((s - 50.0).abs() < 0.01);
        let s2 = compute_outcome_score("add", Some(dec!(100)), Some(dec!(80))).unwrap();
        assert!((s2 + 100.0).abs() < 0.01); // clipped at -100 since -100% * 5 < -100
    }

    #[test]
    fn compute_outcome_score_trim_rewards_down_moves() {
        let s = compute_outcome_score("trim", Some(dec!(100)), Some(dec!(90))).unwrap();
        // -10% * 5 = -50, negated → +50
        assert!((s - 50.0).abs() < 0.01);
    }

    #[test]
    fn compute_outcome_score_hold_rewards_small_moves() {
        let s = compute_outcome_score("hold", Some(dec!(100)), Some(dec!(102))).unwrap();
        assert!((s - 100.0).abs() < 0.01);
        let s2 = compute_outcome_score("hold", Some(dec!(100)), Some(dec!(120))).unwrap();
        assert!((s2 + 100.0).abs() < 0.01);
    }

    #[test]
    fn compute_outcome_score_handles_missing_prices() {
        assert!(compute_outcome_score("add", None, Some(dec!(100))).is_none());
        assert!(compute_outcome_score("add", Some(dec!(100)), None).is_none());
        assert!(compute_outcome_score("add", Some(dec!(0)), Some(dec!(100))).is_none());
    }

    #[test]
    fn link_operator_reply_then_set_score_persists() {
        let conn = fresh_conn();
        let rec_id = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        let reply_id = operator_replies::insert(
            &conn,
            &operator_replies::OperatorReplyInsert {
                journal_id: None,
                report_date: "2026-05-28",
                reply_date: "2026-05-28",
                asset: Some("BTC"),
                decision_type: "add",
                response_class: "yes",
                conviction_implied: None,
                timeframe_horizon: None,
                reasoning_summary: None,
                raw_content: "x",
            },
        )
        .unwrap();
        link_operator_reply(&conn, rec_id, reply_id, Some("accepted")).unwrap();
        set_outcome_score(&conn, rec_id, 42.5, "2026-06-01", Some("auto")).unwrap();
        let out = get_outcome(&conn, rec_id).unwrap().unwrap();
        assert_eq!(out.operator_reply_id, Some(reply_id));
        assert_eq!(out.action_status.as_deref(), Some("accepted"));
        assert!((out.outcome_score.unwrap() - 42.5).abs() < 0.01);
    }

    #[test]
    fn accuracy_summary_counts_hits_and_scored() {
        let conn = fresh_conn();
        // Three add recommendations, two scored above 0, one below.
        for (i, score) in [10.0, 80.0, -25.0].iter().enumerate() {
            let id = insert_recommendation(
                &conn,
                &RecommendationInsert {
                    report_date: "2026-05-28",
                    asset: Some("BTC"),
                    recommendation_type: "add",
                    urgency: "normal",
                    rationale_summary: None,
                },
            )
            .unwrap();
            // Differentiate by id.
            assert_eq!(id, i as i64 + 1);
            set_outcome_score(&conn, id, *score, "2026-06-01", None).unwrap();
        }
        // Plus one unscored trim.
        insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("QQQ"),
                recommendation_type: "trim",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();

        let buckets = accuracy_summary(&conn, None, None, None, 0.0, false).unwrap();
        assert!(!buckets.is_empty());
        let add_bucket = buckets
            .iter()
            .find(|b| b.recommendation_type == "add")
            .unwrap();
        assert_eq!(add_bucket.total, 3);
        assert_eq!(add_bucket.scored, 3);
        assert_eq!(add_bucket.hits, 2);
        let trim_bucket = buckets
            .iter()
            .find(|b| b.recommendation_type == "trim")
            .unwrap();
        assert_eq!(trim_bucket.total, 1);
        assert_eq!(trim_bucket.scored, 0);
    }

    #[test]
    fn rolling_hit_rate_filters_by_window() {
        let conn = fresh_conn();
        let id_old = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-01",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        set_outcome_score(&conn, id_old, -50.0, "2026-05-15", None).unwrap();
        let id_new = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        set_outcome_score(&conn, id_new, 75.0, "2026-06-01", None).unwrap();

        // 7-day window from 2026-06-01 should capture only the 2026-05-28 row.
        let r = rolling_hit_rate(&conn, "2026-06-01", 7, 0.0).unwrap().unwrap();
        assert_eq!(r.scored, 1);
        assert_eq!(r.hits, 1);
        // 60-day window captures both.
        let r2 = rolling_hit_rate(&conn, "2026-06-01", 60, 0.0).unwrap().unwrap();
        assert_eq!(r2.scored, 2);
        assert_eq!(r2.hits, 1);
    }

    #[test]
    fn find_open_for_reply_matches_date_and_asset() {
        let conn = fresh_conn();
        let id = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        let found = find_open_for_reply(&conn, "2026-05-28", Some("BTC"), Some("add"))
            .unwrap()
            .unwrap();
        assert_eq!(found.id, id);
        // After linking, it should no longer be open.
        let reply_id = operator_replies::insert(
            &conn,
            &operator_replies::OperatorReplyInsert {
                journal_id: None,
                report_date: "2026-05-28",
                reply_date: "2026-05-28",
                asset: Some("BTC"),
                decision_type: "add",
                response_class: "yes",
                conviction_implied: None,
                timeframe_horizon: None,
                reasoning_summary: None,
                raw_content: "x",
            },
        )
        .unwrap();
        link_operator_reply(&conn, id, reply_id, Some("accepted")).unwrap();
        assert!(find_open_for_reply(&conn, "2026-05-28", Some("BTC"), Some("add"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn find_open_for_transaction_matches_within_window() {
        let conn = fresh_conn();
        let id = insert_recommendation(
            &conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        // Buy 5 days later matches.
        let m = find_open_for_transaction(&conn, "BTC", "buy", "2026-06-02", 7).unwrap();
        assert_eq!(m.unwrap().id, id);
        // Buy 10 days later does not.
        let m = find_open_for_transaction(&conn, "BTC", "buy", "2026-06-07", 7).unwrap();
        assert!(m.is_none());
        // Sell does not match an 'add' recommendation.
        let m = find_open_for_transaction(&conn, "BTC", "sell", "2026-06-02", 7).unwrap();
        assert!(m.is_none());
    }

    #[test]
    fn parse_decision_reply_extracts_fields() {
        let m = parse_decision_reply("DECISION REPLY asset=BTC type=add response=yes").unwrap();
        assert_eq!(m.asset, "BTC");
        assert_eq!(m.decision_type, "add");
        assert_eq!(m.response_class, "yes");

        let m2 = parse_decision_reply(
            "decision-reply asset=GLD, type=trim, response=no, report_date=2026-05-28",
        )
        .unwrap();
        assert_eq!(m2.asset, "GLD");
        assert_eq!(m2.decision_type, "trim");
        assert_eq!(m2.report_date.as_deref(), Some("2026-05-28"));

        // No payload returns None.
        assert!(parse_decision_reply("just a regular journal entry").is_none());
        // Missing required keys returns None.
        assert!(parse_decision_reply("DECISION REPLY response=yes").is_none());
    }

    // ----- recommendation ledger (gold post-mortem T2) -----

    #[test]
    fn record_ledger_entry_autofills_entry_price_from_history() {
        let conn = fresh_conn();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('GC=F', '2026-06-08', '3300.50'),
                ('GC=F', '2026-06-09', '3310.25');",
        )
        .unwrap();
        let rec = record_ledger_entry(
            &conn,
            "2026-06-10",
            "gc=f",
            "add",
            Some("accumulation window open"),
            "decision-architect",
        )
        .unwrap();
        assert_eq!(rec.asset.as_deref(), Some("GC=F"));
        assert_eq!(rec.recommendation_type, "add");
        // Closest close on/before run_date.
        assert_eq!(rec.entry_price.as_deref(), Some("3310.25"));
        assert_eq!(rec.price_series.as_deref(), Some("GC=F"));
        assert_eq!(rec.source, "decision-architect");
        assert!(rec.scored_at.is_none());
    }

    #[test]
    fn record_ledger_entry_falls_back_to_usd_series() {
        let conn = fresh_conn();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('BTC-USD', '2026-06-09', '101000.00');",
        )
        .unwrap();
        let rec = record_ledger_entry(&conn, "2026-06-10", "BTC", "wait", None, "decision-architect")
            .unwrap();
        assert_eq!(rec.entry_price.as_deref(), Some("101000.00"));
        assert_eq!(rec.price_series.as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn record_ledger_entry_without_history_records_unpriced() {
        let conn = fresh_conn();
        let rec =
            record_ledger_entry(&conn, "2026-06-10", "XYZ", "hold", None, "decision-architect")
                .unwrap();
        assert!(rec.entry_price.is_none());
        assert!(rec.price_series.is_none());
    }

    #[test]
    fn record_ledger_entry_rejects_non_ledger_actions() {
        let conn = fresh_conn();
        assert!(
            record_ledger_entry(&conn, "2026-06-10", "BTC", "exit", None, "x").is_err(),
            "exit is a decision-card type, not a ledger action"
        );
        assert!(record_ledger_entry(&conn, "2026-06-10", "BTC", "smash", None, "x").is_err());
    }

    #[test]
    fn score_forward_returns_fills_elapsed_horizons_only() {
        let conn = fresh_conn();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('GC=F', '2026-01-01', '100.00'),
                ('GC=F', '2026-01-31', '110.00'),
                ('GC=F', '2026-04-01', '90.00');",
        )
        .unwrap();
        let rec =
            record_ledger_entry(&conn, "2026-01-01", "GC=F", "add", None, "decision-architect")
                .unwrap();
        assert_eq!(rec.entry_price.as_deref(), Some("100.00"));

        // 100 days later: 30d and 90d elapsed, 180d not.
        let summary = score_forward_returns(&conn, "2026-04-11").unwrap();
        assert_eq!(summary.rows_updated, 1);
        assert_eq!(summary.horizons_filled, 2);
        let row = get(&conn, rec.id).unwrap().unwrap();
        assert!((row.fwd_30d_pct.unwrap() - 10.0).abs() < 1e-9); // 100 → 110
        assert!((row.fwd_90d_pct.unwrap() + 10.0).abs() < 1e-9); // 100 → 90 (close on/before +90d)
        assert!(row.fwd_180d_pct.is_none());
        assert!(row.scored_at.is_some());
    }

    #[test]
    fn score_forward_returns_is_idempotent_and_never_overwrites() {
        let conn = fresh_conn();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('GC=F', '2026-01-01', '100.00'),
                ('GC=F', '2026-01-31', '110.00');",
        )
        .unwrap();
        let rec =
            record_ledger_entry(&conn, "2026-01-01", "GC=F", "add", None, "decision-architect")
                .unwrap();
        let s1 = score_forward_returns(&conn, "2026-02-15").unwrap();
        assert_eq!(s1.horizons_filled, 1);
        // Mutate price history; a re-run must NOT change the scored horizon.
        conn.execute(
            "UPDATE price_history SET close = '200.00' WHERE symbol = 'GC=F' AND date = '2026-01-31'",
            [],
        )
        .unwrap();
        let s2 = score_forward_returns(&conn, "2026-02-15").unwrap();
        assert_eq!(s2.horizons_filled, 0);
        let row = get(&conn, rec.id).unwrap().unwrap();
        assert!((row.fwd_30d_pct.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn score_forward_returns_requires_close_after_run_date() {
        let conn = fresh_conn();
        // Only the entry close exists — the on-or-before lookup must NOT
        // score the row against its own entry close.
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('GC=F', '2026-01-01', '100.00');",
        )
        .unwrap();
        record_ledger_entry(&conn, "2026-01-01", "GC=F", "add", None, "decision-architect")
            .unwrap();
        let s = score_forward_returns(&conn, "2026-12-01").unwrap();
        assert_eq!(s.horizons_filled, 0);
    }

    #[test]
    fn scoreboard_aggregates_mix_and_window_quality() {
        let conn = fresh_conn();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES ('GC=F','2026-01-01','100.00');",
        )
        .unwrap();
        // Two scored ADDs (mean 90d = -10%), one scored WAIT (90d = +5%),
        // one unscored ADD still accruing.
        for (action, f90) in [("add", Some(-15.0)), ("add", Some(-5.0)), ("wait", Some(5.0)), ("add", None)] {
            let rec = record_ledger_entry(&conn, "2026-01-01", "GC=F", action, None, "decision-architect")
                .unwrap();
            if let Some(v) = f90 {
                conn.execute(
                    "UPDATE recommendations SET fwd_90d_pct = ?1, scored_at = datetime('now') WHERE id = ?2",
                    params![v, rec.id],
                )
                .unwrap();
            }
        }
        let board = scoreboard(&conn, Some("GC=F")).unwrap();
        let add_row = board
            .rows
            .iter()
            .find(|r| r.action == "add")
            .expect("add row");
        assert_eq!(add_row.n_total, 3);
        let h90 = add_row.h90.as_ref().unwrap();
        assert_eq!(h90.n, 2);
        assert_eq!(h90.positive, 0);
        assert!((h90.mean_pct + 10.0).abs() < 1e-9);
        let wait_row = board.rows.iter().find(|r| r.action == "wait").unwrap();
        assert_eq!(wait_row.h90.as_ref().unwrap().n, 1);

        // Window quality: ADD mean (-10) − WAIT mean (+5) = -15 → the gold
        // failure made measurable.
        assert_eq!(board.window_quality.len(), 1);
        let wq = &board.window_quality[0];
        assert_eq!(wq.symbol, "GC=F");
        assert_eq!(wq.add_n, 2);
        assert_eq!(wq.wait_n, 1);
        assert!((wq.delta_pct.unwrap() + 15.0).abs() < 1e-9);

        assert_eq!(board.unscored, 1);
    }

    #[test]
    fn scoreboard_accruing_state_with_no_scored_rows() {
        let conn = fresh_conn();
        record_ledger_entry(&conn, "2026-06-10", "SI=F", "wait", None, "decision-architect")
            .unwrap();
        let board = scoreboard(&conn, None).unwrap();
        assert_eq!(board.unscored, 1);
        let row = &board.rows[0];
        assert!(row.h30.is_none() && row.h90.is_none() && row.h180.is_none());
        assert!(board.window_quality.iter().all(|w| w.delta_pct.is_none()));
    }

    #[test]
    fn ensure_table_migrates_legacy_shape_additively() {
        // A pre-ledger DB: old-shape recommendations table without the new
        // columns. ensure_table must self-heal it.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE recommendations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                report_date TEXT NOT NULL,
                asset TEXT,
                recommendation_type TEXT NOT NULL,
                urgency TEXT NOT NULL,
                rationale_summary TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                PRIMARY KEY (symbol, date)
            );
            INSERT INTO recommendations (report_date, asset, recommendation_type, urgency)
            VALUES ('2026-05-01', 'BTC', 'add', 'normal');",
        )
        .unwrap();
        ensure_table(&conn).unwrap();
        // Legacy row readable through the new mapper, with defaults applied.
        let rows = list(&conn, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "decision-architect");
        assert!(rows[0].entry_price.is_none());
        // And new writes work.
        record_ledger_entry(&conn, "2026-06-10", "BTC", "wait", None, "decision-architect")
            .unwrap();
    }

    #[test]
    fn score_from_price_history_uses_close_lookup() {
        let conn = fresh_conn();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('BTC', '2026-05-28', '100.00'),
                ('BTC', '2026-06-11', '110.00');",
        )
        .unwrap();
        let start = price_at(&conn, "BTC", "2026-05-28").unwrap();
        let end = price_at(&conn, "BTC", "2026-06-11").unwrap();
        let s = compute_outcome_score("add", start, end).unwrap();
        assert!((s - 50.0).abs() < 0.01);
    }
}
