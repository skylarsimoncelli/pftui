use anyhow::{anyhow, Result};
use rusqlite::Connection;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

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

#[derive(Debug, Clone)]
struct RuleColumns {
    id: String,
    prediction_id: String,
    rule_type: String,
    symbol: Option<String>,
    threshold_value: Option<String>,
    threshold_low: Option<String>,
    threshold_high: Option<String>,
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
    Ok(())
}

pub fn ensure_table_backend(backend: &BackendConnection) -> Result<()> {
    query::dispatch(backend, ensure_table, ensure_table_postgres)
}

pub fn list_due_auto_score_rules_backend(
    backend: &BackendConnection,
    since: Option<&str>,
    today: &str,
) -> Result<Vec<PredictionFalsificationRule>> {
    query::dispatch(
        backend,
        |conn| list_due_auto_score_rules(conn, since, today),
        |pool| list_due_auto_score_rules_postgres(pool, since, today),
    )
}

fn list_due_auto_score_rules(
    conn: &Connection,
    since: Option<&str>,
    today: &str,
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

    let mut where_parts = vec![format!("r.{} <= ?1", columns.eval_date_end)];
    if let Some(eligible) = columns.auto_score_eligible.as_ref() {
        where_parts.push(format!("COALESCE(r.{eligible}, 0) = 1"));
    }
    if since.is_some() {
        where_parts.push(format!("r.{} >= ?2", columns.eval_date_end));
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
        stmt.query(rusqlite::params![today, since])?
    } else {
        stmt.query(rusqlite::params![today])?
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
        id: required(&["id"])?,
        prediction_id: required(&["prediction_id", "user_prediction_id"])?,
        rule_type: required(&["rule_type"])?,
        symbol: optional(&names, &["symbol", "asset_symbol", "ticker"]),
        threshold_value: optional(&names, &["threshold_value", "threshold", "target_value"]),
        threshold_low: optional(
            &names,
            &[
                "threshold_low",
                "lower_threshold",
                "threshold_min",
                "min_value",
            ],
        ),
        threshold_high: optional(
            &names,
            &[
                "threshold_high",
                "upper_threshold",
                "threshold_max",
                "max_value",
            ],
        ),
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

fn list_due_auto_score_rules_postgres(
    pool: &PgPool,
    since: Option<&str>,
    today: &str,
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
               AND r.eval_date_end <= ",
        );
        query.push_bind(today);
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
