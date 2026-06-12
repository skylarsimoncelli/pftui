use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Read-only row shape returned by `list()` for the
/// `pftui analytics falsifications` CLI. Mirrors the columns actually
/// present in the live DB / `ensure_table` schema below.
#[derive(Debug, Clone, Serialize)]
pub struct FalsificationRuleSummary {
    pub id: i64,
    pub prediction_id: Option<i64>,
    pub rule_type: String,
    pub symbol: Option<String>,
    pub threshold_value: Option<f64>,
    pub threshold_low: Option<f64>,
    pub threshold_high: Option<f64>,
    pub eval_date_start: Option<String>,
    pub eval_date_end: Option<String>,
    pub parse_confidence: Option<String>,
    pub auto_score_eligible: bool,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PredictionFalsificationRule {
    pub id: i64,
    pub prediction_id: i64,
    pub claim: String,
    pub prediction_symbol: Option<String>,
    pub current_outcome: String,
    pub rule_type: String,
    pub symbol: Option<String>,
    pub threshold_value: Option<f64>,
    pub threshold_low: Option<f64>,
    pub threshold_high: Option<f64>,
    pub eval_date_start: Option<String>,
    pub eval_date_end: String,
    pub parse_confidence: String,
}

/// Write-shape for a new falsification rule. Column names are resolved
/// against the live table at insert time (`detect_columns`) so both the
/// repo shape (`symbol`/`threshold_value`/`threshold_low`/`threshold_high`)
/// and the alternate deployed shape (`asset`/`threshold_lower`/
/// `threshold_upper`/`threshold_text`) are supported.
#[derive(Debug, Clone, Serialize)]
pub struct NewFalsificationRule {
    pub prediction_id: i64,
    pub rule_type: String,
    pub symbol: Option<String>,
    pub threshold_value: Option<f64>,
    pub threshold_low: Option<f64>,
    pub threshold_high: Option<f64>,
    pub threshold_text: Option<String>,
    pub eval_date_start: String,
    pub eval_date_end: String,
    pub auto_score_eligible: bool,
    pub parse_confidence: String,
}

#[derive(Debug, Clone)]
struct RuleColumns {
    id: String,
    prediction_id: String,
    rule_type: String,
    symbol: Option<String>,
    threshold_value: Option<String>,
    threshold_low: Option<String>,
    threshold_high: Option<String>,
    threshold_text: Option<String>,
    eval_date_start: Option<String>,
    eval_date_end: String,
    parse_confidence: Option<String>,
    auto_score_eligible: Option<String>,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prediction_falsification_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER NOT NULL REFERENCES user_predictions(id) ON DELETE CASCADE,
            rule_type TEXT NOT NULL,
            symbol TEXT,
            threshold_value REAL,
            threshold_low REAL,
            threshold_high REAL,
            threshold_text TEXT,
            eval_date_start TEXT,
            eval_date_end TEXT NOT NULL,
            parse_confidence TEXT NOT NULL DEFAULT 'medium',
            auto_score_eligible INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_prediction_falsification_rules_auto
            ON prediction_falsification_rules(auto_score_eligible, eval_date_end, parse_confidence);
        CREATE INDEX IF NOT EXISTS idx_prediction_falsification_rules_prediction
            ON prediction_falsification_rules(prediction_id);",
    )?;
    // Self-heal pre-existing tables that lack the raw-text column used by
    // unstructured (`rule_type='unstructured'`) rules.
    let has_text_col: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('prediction_falsification_rules')
             WHERE name = 'threshold_text'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_text_col {
        conn.execute(
            "ALTER TABLE prediction_falsification_rules ADD COLUMN threshold_text TEXT",
            [],
        )?;
    }
    Ok(())
}

/// Insert a falsification rule, tolerating both known on-disk column
/// shapes. Single-threshold rules write `threshold_value` when that column
/// exists, otherwise fall back to the lower-threshold column so the value
/// is never silently dropped.
pub fn insert_rule(conn: &Connection, rule: &NewFalsificationRule) -> Result<i64> {
    ensure_table(conn)?;
    let columns = detect_columns(conn)?;

    let mut cols: Vec<String> = vec![
        columns.prediction_id.clone(),
        columns.rule_type.clone(),
        columns.eval_date_end.clone(),
    ];
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(rule.prediction_id),
        Box::new(rule.rule_type.clone()),
        Box::new(rule.eval_date_end.clone()),
    ];

    if let Some(col) = &columns.symbol {
        cols.push(col.clone());
        values.push(Box::new(rule.symbol.clone()));
    }
    let mut threshold_low = rule.threshold_low;
    if let Some(col) = &columns.threshold_value {
        cols.push(col.clone());
        values.push(Box::new(rule.threshold_value));
    } else if threshold_low.is_none() {
        // Alternate shape without threshold_value: keep the single
        // threshold in the lower-bound column.
        threshold_low = rule.threshold_value;
    }
    if let Some(col) = &columns.threshold_low {
        cols.push(col.clone());
        values.push(Box::new(threshold_low));
    }
    if let Some(col) = &columns.threshold_high {
        cols.push(col.clone());
        values.push(Box::new(rule.threshold_high));
    }
    if let Some(col) = &columns.threshold_text {
        cols.push(col.clone());
        values.push(Box::new(rule.threshold_text.clone()));
    }
    if let Some(col) = &columns.eval_date_start {
        cols.push(col.clone());
        values.push(Box::new(rule.eval_date_start.clone()));
    }
    if let Some(col) = &columns.parse_confidence {
        cols.push(col.clone());
        values.push(Box::new(rule.parse_confidence.clone()));
    }
    if let Some(col) = &columns.auto_score_eligible {
        cols.push(col.clone());
        values.push(Box::new(rule.auto_score_eligible as i64));
    }

    let placeholders = (1..=cols.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO prediction_falsification_rules ({}) VALUES ({})",
        cols.join(", "),
        placeholders
    );
    let params_slice: Vec<&dyn rusqlite::ToSql> = values.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, params_slice.as_slice())?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_rule_backend(
    backend: &BackendConnection,
    rule: &NewFalsificationRule,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| insert_rule(conn, rule),
        |pool| insert_rule_postgres(pool, rule),
    )
}

/// Generic list for the `pftui analytics falsifications` CLI. Filters by
/// rule type, auto-eligibility, and owning prediction. Tolerant of column
/// drift between schema versions.
pub fn list(
    conn: &Connection,
    rule_type: Option<&str>,
    auto_eligible_only: bool,
    for_prediction: Option<i64>,
) -> Result<Vec<FalsificationRuleSummary>> {
    ensure_table(conn)?;
    let columns = detect_columns(conn)?;
    let symbol_expr = columns
        .symbol
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let threshold_value_expr = columns
        .threshold_value
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let threshold_low_expr = columns
        .threshold_low
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let threshold_high_expr = columns
        .threshold_high
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let eval_start_expr = columns
        .eval_date_start
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let parse_confidence_expr = columns
        .parse_confidence
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let auto_eligible_expr = columns
        .auto_score_eligible
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "0".to_string());

    let mut sql = format!(
        "SELECT r.{id} AS id, r.{prediction_id} AS prediction_id, r.{rule_type} AS rule_type,
                {symbol} AS symbol, {threshold_value} AS threshold_value,
                {threshold_low} AS threshold_low, {threshold_high} AS threshold_high,
                {eval_start} AS eval_date_start, r.{eval_end} AS eval_date_end,
                {parse_confidence} AS parse_confidence,
                {auto_eligible} AS auto_score_eligible
         FROM prediction_falsification_rules r WHERE 1=1",
        id = columns.id,
        prediction_id = columns.prediction_id,
        rule_type = columns.rule_type,
        symbol = symbol_expr,
        threshold_value = threshold_value_expr,
        threshold_low = threshold_low_expr,
        threshold_high = threshold_high_expr,
        eval_start = eval_start_expr,
        eval_end = columns.eval_date_end,
        parse_confidence = parse_confidence_expr,
        auto_eligible = auto_eligible_expr,
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(t) = rule_type {
        sql.push_str(&format!(" AND r.{} = ?", columns.rule_type));
        args.push(Box::new(t.to_string()));
    }
    if auto_eligible_only {
        if let Some(col) = &columns.auto_score_eligible {
            sql.push_str(&format!(" AND r.{} = 1", col));
        }
    }
    if let Some(p) = for_prediction {
        sql.push_str(&format!(" AND r.{} = ?", columns.prediction_id));
        args.push(Box::new(p));
    }
    sql.push_str(&format!(" ORDER BY r.{} DESC", columns.id));

    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(FalsificationRuleSummary {
                id: row.get(0)?,
                prediction_id: row.get(1).ok(),
                rule_type: row.get(2)?,
                symbol: row.get(3).ok().flatten(),
                threshold_value: row.get(4).ok().flatten(),
                threshold_low: row.get(5).ok().flatten(),
                threshold_high: row.get(6).ok().flatten(),
                eval_date_start: row.get(7).ok().flatten(),
                eval_date_end: row.get(8).ok(),
                parse_confidence: row.get(9).ok().flatten(),
                auto_score_eligible: row.get::<_, Option<i64>>(10).ok().flatten().unwrap_or(0)
                    != 0,
                created_at: None,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn ensure_table_backend(backend: &BackendConnection) -> Result<()> {
    query::dispatch(backend, ensure_table, ensure_table_postgres)
}

/// List every auto-score-eligible rule regardless of whether its evaluation
/// window has closed. Open-window rules can still be decided early:
/// `close-*`/`prints-*` rules score CORRECT as soon as one qualifying close
/// prints, and `stays-*` rules score WRONG on the first violating close.
pub fn list_active_auto_score_rules_backend(
    backend: &BackendConnection,
    since: Option<&str>,
) -> Result<Vec<PredictionFalsificationRule>> {
    query::dispatch(
        backend,
        |conn| list_active_auto_score_rules(conn, since),
        |pool| list_active_auto_score_rules_postgres(pool, since),
    )
}

fn list_active_auto_score_rules(
    conn: &Connection,
    since: Option<&str>,
) -> Result<Vec<PredictionFalsificationRule>> {
    ensure_table(conn)?;
    let columns = detect_columns(conn)?;

    let symbol_expr = columns
        .symbol
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let threshold_value_expr = columns
        .threshold_value
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let threshold_low_expr = columns
        .threshold_low
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let threshold_high_expr = columns
        .threshold_high
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let eval_start_expr = columns
        .eval_date_start
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let parse_confidence_expr = columns
        .parse_confidence
        .as_ref()
        .map(|c| format!("r.{c}"))
        .unwrap_or_else(|| "'medium'".to_string());

    let mut where_parts = vec!["r.rule_type != 'unstructured'".to_string()];
    if let Some(eligible) = columns.auto_score_eligible.as_ref() {
        where_parts.push(format!("COALESCE(r.{eligible}, 0) = 1"));
    }
    if since.is_some() {
        where_parts.push(format!("r.{} >= ?1", columns.eval_date_end));
    }

    let sql = format!(
        "SELECT
            r.{id},
            r.{prediction_id},
            p.claim,
            p.symbol,
            p.outcome,
            r.{rule_type},
            {symbol_expr},
            {threshold_value_expr},
            {threshold_low_expr},
            {threshold_high_expr},
            {eval_start_expr},
            r.{eval_date_end},
            {parse_confidence_expr}
         FROM prediction_falsification_rules r
         JOIN user_predictions p ON p.id = r.{prediction_id}
         WHERE {where_clause}
         ORDER BY r.{eval_date_end} ASC, r.{id} ASC",
        id = columns.id,
        prediction_id = columns.prediction_id,
        rule_type = columns.rule_type,
        eval_date_end = columns.eval_date_end,
        where_clause = where_parts.join(" AND "),
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = if let Some(since) = since {
        stmt.query(rusqlite::params![since])?
    } else {
        stmt.query([])?
    };

    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(PredictionFalsificationRule {
            id: row.get(0)?,
            prediction_id: row.get(1)?,
            claim: row.get(2)?,
            prediction_symbol: row.get(3)?,
            current_outcome: row.get(4)?,
            rule_type: row.get(5)?,
            symbol: row.get(6)?,
            threshold_value: row.get(7)?,
            threshold_low: row.get(8)?,
            threshold_high: row.get(9)?,
            eval_date_start: row.get(10)?,
            eval_date_end: row.get(11)?,
            parse_confidence: row
                .get::<_, Option<String>>(12)?
                .unwrap_or_else(|| "medium".into()),
        });
    }
    Ok(out)
}

/// List EVERY falsification rule joined to its prediction, regardless of
/// auto-score eligibility, rule type, or current outcome. Used by the
/// legacy-outcome rescore audit (`journal prediction rescore-audit`), which
/// must see event-*/unstructured rows too (they classify as unparseable)
/// and re-evaluate rules on predictions that are already scored.
/// Tolerates both on-disk column shapes via `detect_columns`.
pub fn list_all_rules(conn: &Connection) -> Result<Vec<PredictionFalsificationRule>> {
    ensure_table(conn)?;
    let columns = detect_columns(conn)?;

    let opt_expr = |col: &Option<String>, fallback: &str| -> String {
        col.as_ref()
            .map(|c| format!("r.{c}"))
            .unwrap_or_else(|| fallback.to_string())
    };
    let sql = format!(
        "SELECT
            r.{id},
            r.{prediction_id},
            p.claim,
            p.symbol,
            p.outcome,
            r.{rule_type},
            {symbol_expr},
            {threshold_value_expr},
            {threshold_low_expr},
            {threshold_high_expr},
            {eval_start_expr},
            r.{eval_date_end},
            {parse_confidence_expr}
         FROM prediction_falsification_rules r
         JOIN user_predictions p ON p.id = r.{prediction_id}
         ORDER BY r.{prediction_id} ASC",
        id = columns.id,
        prediction_id = columns.prediction_id,
        rule_type = columns.rule_type,
        symbol_expr = opt_expr(&columns.symbol, "NULL"),
        threshold_value_expr = opt_expr(&columns.threshold_value, "NULL"),
        threshold_low_expr = opt_expr(&columns.threshold_low, "NULL"),
        threshold_high_expr = opt_expr(&columns.threshold_high, "NULL"),
        eval_start_expr = opt_expr(&columns.eval_date_start, "NULL"),
        eval_date_end = columns.eval_date_end,
        parse_confidence_expr = opt_expr(&columns.parse_confidence, "'medium'"),
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(PredictionFalsificationRule {
            id: row.get(0)?,
            prediction_id: row.get(1)?,
            claim: row.get(2)?,
            prediction_symbol: row.get(3)?,
            current_outcome: row.get(4)?,
            rule_type: row.get(5)?,
            symbol: row.get(6)?,
            threshold_value: row.get(7)?,
            threshold_low: row.get(8)?,
            threshold_high: row.get(9)?,
            eval_date_start: row.get(10)?,
            eval_date_end: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
            parse_confidence: row
                .get::<_, Option<String>>(12)?
                .unwrap_or_else(|| "medium".into()),
        });
    }
    Ok(out)
}

fn detect_columns(conn: &Connection) -> Result<RuleColumns> {
    let mut stmt = conn.prepare("PRAGMA table_info('prediction_falsification_rules')")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut names = Vec::new();
    for row in rows {
        names.push(row?);
    }

    let required = |candidates: &[&str]| -> Result<String> {
        optional(&names, candidates).ok_or_else(|| {
            anyhow!(
                "prediction_falsification_rules is missing required column; expected one of {:?}",
                candidates
            )
        })
    };

    Ok(RuleColumns {
        id: required(&["id", "prediction_id"])?,
        prediction_id: required(&["prediction_id", "user_prediction_id"])?,
        rule_type: required(&["rule_type"])?,
        symbol: optional(&names, &["symbol", "asset", "asset_symbol", "ticker"]),
        threshold_value: optional(&names, &["threshold_value", "threshold", "target_value"]),
        threshold_low: optional(
            &names,
            &[
                "threshold_low",
                "threshold_lower",
                "lower_threshold",
                "threshold_min",
                "min_value",
            ],
        ),
        threshold_high: optional(
            &names,
            &[
                "threshold_high",
                "threshold_upper",
                "upper_threshold",
                "threshold_max",
                "max_value",
            ],
        ),
        threshold_text: optional(&names, &["threshold_text"]),
        eval_date_start: optional(&names, &["eval_date_start", "start_date", "window_start"]),
        eval_date_end: required(&["eval_date_end", "end_date", "window_end", "target_date"])?,
        parse_confidence: optional(&names, &["parse_confidence", "confidence"]),
        auto_score_eligible: optional(&names, &["auto_score_eligible", "autoscore_eligible"]),
    })
}

fn optional(names: &[String], candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| names.iter().any(|name| name == **candidate))
        .map(|value| (*value).to_string())
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS prediction_falsification_rules (
                id BIGSERIAL PRIMARY KEY,
                prediction_id BIGINT NOT NULL REFERENCES user_predictions(id) ON DELETE CASCADE,
                rule_type TEXT NOT NULL,
                symbol TEXT,
                threshold_value DOUBLE PRECISION,
                threshold_low DOUBLE PRECISION,
                threshold_high DOUBLE PRECISION,
                threshold_text TEXT,
                eval_date_start TEXT,
                eval_date_end TEXT NOT NULL,
                parse_confidence TEXT NOT NULL DEFAULT 'medium',
                auto_score_eligible BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE prediction_falsification_rules
                ADD COLUMN IF NOT EXISTS threshold_text TEXT",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_prediction_falsification_rules_auto
                ON prediction_falsification_rules(auto_score_eligible, eval_date_end, parse_confidence)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_prediction_falsification_rules_prediction
                ON prediction_falsification_rules(prediction_id)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_rule_postgres(pool: &PgPool, rule: &NewFalsificationRule) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO prediction_falsification_rules
                (prediction_id, rule_type, symbol, threshold_value, threshold_low,
                 threshold_high, threshold_text, eval_date_start, eval_date_end,
                 parse_confidence, auto_score_eligible)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             RETURNING id",
        )
        .bind(rule.prediction_id)
        .bind(&rule.rule_type)
        .bind(&rule.symbol)
        .bind(rule.threshold_value)
        .bind(rule.threshold_low)
        .bind(rule.threshold_high)
        .bind(&rule.threshold_text)
        .bind(&rule.eval_date_start)
        .bind(&rule.eval_date_end)
        .bind(&rule.parse_confidence)
        .bind(rule.auto_score_eligible)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_active_auto_score_rules_postgres(
    pool: &PgPool,
    since: Option<&str>,
) -> Result<Vec<PredictionFalsificationRule>> {
    ensure_table_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        let mut query = sqlx::QueryBuilder::new(
            "SELECT
                r.id,
                r.prediction_id,
                p.claim,
                p.symbol,
                p.outcome,
                r.rule_type,
                r.symbol,
                r.threshold_value,
                r.threshold_low,
                r.threshold_high,
                r.eval_date_start,
                r.eval_date_end,
                r.parse_confidence
             FROM prediction_falsification_rules r
             JOIN user_predictions p ON p.id = r.prediction_id
             WHERE r.auto_score_eligible = TRUE
               AND r.rule_type != 'unstructured'",
        );
        if let Some(since) = since {
            query.push(" AND r.eval_date_end >= ");
            query.push_bind(since);
        }
        query.push(" ORDER BY r.eval_date_end ASC, r.id ASC");
        query
            .build_query_as::<(
                i64,
                i64,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                Option<f64>,
                Option<f64>,
                Option<f64>,
                Option<String>,
                String,
                String,
            )>()
            .fetch_all(pool)
            .await
    })?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                prediction_id,
                claim,
                prediction_symbol,
                current_outcome,
                rule_type,
                symbol,
                threshold_value,
                threshold_low,
                threshold_high,
                eval_date_start,
                eval_date_end,
                parse_confidence,
            )| PredictionFalsificationRule {
                id,
                prediction_id,
                claim,
                prediction_symbol,
                current_outcome,
                rule_type,
                symbol,
                threshold_value,
                threshold_low,
                threshold_high,
                eval_date_start,
                eval_date_end,
                parse_confidence,
            },
        )
        .collect())
}
