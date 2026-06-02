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
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Recommendation types accepted by the action-status pipeline. Aligned with
/// `operator_replies::VALID_DECISION_TYPES` so the auto-linker can match the
/// recommendation that produced a given reply.
pub const VALID_RECOMMENDATION_TYPES: &[&str] = &[
    "add",
    "trim",
    "hold",
    "exit",
    "target-set",
    "target-remove",
    "target-ignore",
    "outlook-refine",
    "catalyst",
    "meta",
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
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
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
    Ok(())
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
    let mut sql = String::from(
        "SELECT id, report_date, asset, recommendation_type, urgency,
                rationale_summary, created_at
         FROM recommendations WHERE 1=1",
    );
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
        .query_map(params_slice.as_slice(), |row| {
            Ok(Recommendation {
                id: row.get(0)?,
                report_date: row.get(1)?,
                asset: row.get(2)?,
                recommendation_type: row.get(3)?,
                urgency: row.get(4)?,
                rationale_summary: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Recommendation>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, report_date, asset, recommendation_type, urgency,
                rationale_summary, created_at
         FROM recommendations WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |row| {
            Ok(Recommendation {
                id: row.get(0)?,
                report_date: row.get(1)?,
                asset: row.get(2)?,
                recommendation_type: row.get(3)?,
                urgency: row.get(4)?,
                rationale_summary: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .ok();
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
    let mut sql = String::from(
        "SELECT r.id, r.report_date, r.asset, r.recommendation_type, r.urgency,
                r.rationale_summary, r.created_at
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE r.report_date = ?1
           AND o.operator_reply_id IS NULL",
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
        .query_row(params_slice.as_slice(), |row| {
            Ok(Recommendation {
                id: row.get(0)?,
                report_date: row.get(1)?,
                asset: row.get(2)?,
                recommendation_type: row.get(3)?,
                urgency: row.get(4)?,
                rationale_summary: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
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
        "SELECT r.id, r.report_date, r.asset, r.recommendation_type, r.urgency,
                r.rationale_summary, r.created_at
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE r.asset = ?1
           AND r.recommendation_type IN ({})
           AND r.report_date <= ?2
           AND julianday(?2) - julianday(r.report_date) <= ?3
           AND (o.transaction_id IS NULL)
         ORDER BY r.report_date DESC LIMIT 1",
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
        .query_row(params_slice.as_slice(), |row| {
            Ok(Recommendation {
                id: row.get(0)?,
                report_date: row.get(1)?,
                asset: row.get(2)?,
                recommendation_type: row.get(3)?,
                urgency: row.get(4)?,
                rationale_summary: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .ok();
    Ok(row)
}

/// List all recommendations that do not yet have a stored outcome score.
/// Useful for `pftui analytics recommendations score --all`.
pub fn list_unscored(conn: &Connection, since: Option<&str>) -> Result<Vec<Recommendation>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT r.id, r.report_date, r.asset, r.recommendation_type, r.urgency,
                r.rationale_summary, r.created_at
         FROM recommendations r
         LEFT JOIN recommendation_outcomes o ON o.recommendation_id = r.id
         WHERE (o.outcome_score IS NULL)",
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
        .query_map(params_slice.as_slice(), |row| {
            Ok(Recommendation {
                id: row.get(0)?,
                report_date: row.get(1)?,
                asset: row.get(2)?,
                recommendation_type: row.get(3)?,
                urgency: row.get(4)?,
                rationale_summary: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
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
