//! `operator_replies` — structured per-decision replies the operator writes
//! against report content. Bidirectional with the DECISION REPLY journaling
//! flow. Mirrored from the live-DB enrichment session (June 1 2026).

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const VALID_DECISION_TYPES: &[&str] = &[
    "add",
    "trim",
    "hold",
    "exit",
    "target-set",
    "target-remove",
    "target-ignore",
    "executed",
    "outlook-refine",
    "meta",
];

pub const VALID_RESPONSE_CLASSES: &[&str] = &[
    "yes", "no", "wait", "refine", "remove", "executed", "ignore",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperatorReply {
    pub id: i64,
    pub journal_id: Option<i64>,
    pub report_date: String,
    pub reply_date: String,
    pub asset: Option<String>,
    pub decision_type: String,
    pub response_class: String,
    pub conviction_implied: Option<String>,
    pub timeframe_horizon: Option<String>,
    pub reasoning_summary: Option<String>,
    pub raw_content: String,
}

#[derive(Debug, Clone)]
pub struct OperatorReplyInsert<'a> {
    pub journal_id: Option<i64>,
    pub report_date: &'a str,
    pub reply_date: &'a str,
    pub asset: Option<&'a str>,
    pub decision_type: &'a str,
    pub response_class: &'a str,
    pub conviction_implied: Option<&'a str>,
    pub timeframe_horizon: Option<&'a str>,
    pub reasoning_summary: Option<&'a str>,
    pub raw_content: &'a str,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS operator_replies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            journal_id INTEGER,
            report_date TEXT NOT NULL,
            reply_date TEXT NOT NULL,
            asset TEXT,
            decision_type TEXT NOT NULL CHECK(decision_type IN (
                'add','trim','hold','exit','target-set','target-remove',
                'target-ignore','executed','outlook-refine','meta'
            )),
            response_class TEXT NOT NULL CHECK(response_class IN (
                'yes','no','wait','refine','remove','executed','ignore'
            )),
            conviction_implied TEXT,
            timeframe_horizon TEXT,
            reasoning_summary TEXT,
            raw_content TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_operator_replies_report_date
            ON operator_replies(report_date);
        CREATE INDEX IF NOT EXISTS idx_operator_replies_asset
            ON operator_replies(asset);
        CREATE INDEX IF NOT EXISTS idx_operator_replies_decision_type
            ON operator_replies(decision_type);",
    )?;
    Ok(())
}

pub fn list(
    conn: &Connection,
    report_date: Option<&str>,
    asset: Option<&str>,
    decision_type: Option<&str>,
) -> Result<Vec<OperatorReply>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, journal_id, report_date, reply_date, asset, decision_type,
                response_class, conviction_implied, timeframe_horizon,
                reasoning_summary, raw_content
         FROM operator_replies WHERE 1=1",
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
    if let Some(t) = decision_type {
        sql.push_str(" AND decision_type = ?");
        args.push(Box::new(t.to_string()));
    }
    sql.push_str(" ORDER BY reply_date DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(OperatorReply {
                id: row.get(0)?,
                journal_id: row.get(1)?,
                report_date: row.get(2)?,
                reply_date: row.get(3)?,
                asset: row.get(4)?,
                decision_type: row.get(5)?,
                response_class: row.get(6)?,
                conviction_implied: row.get(7)?,
                timeframe_horizon: row.get(8)?,
                reasoning_summary: row.get(9)?,
                raw_content: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn insert(conn: &Connection, row: &OperatorReplyInsert<'_>) -> Result<i64> {
    ensure_table(conn)?;
    if !VALID_DECISION_TYPES.contains(&row.decision_type) {
        return Err(anyhow!(
            "invalid decision_type '{}'; must be one of {}",
            row.decision_type,
            VALID_DECISION_TYPES.join("|")
        ));
    }
    if !VALID_RESPONSE_CLASSES.contains(&row.response_class) {
        return Err(anyhow!(
            "invalid response_class '{}'; must be one of {}",
            row.response_class,
            VALID_RESPONSE_CLASSES.join("|")
        ));
    }
    conn.execute(
        "INSERT INTO operator_replies
            (journal_id, report_date, reply_date, asset, decision_type,
             response_class, conviction_implied, timeframe_horizon,
             reasoning_summary, raw_content)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            row.journal_id,
            row.report_date,
            row.reply_date,
            row.asset,
            row.decision_type,
            row.response_class,
            row.conviction_implied,
            row.timeframe_horizon,
            row.reasoning_summary,
            row.raw_content,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_then_list_roundtrips() {
        let conn = fresh_conn();
        let id = insert(
            &conn,
            &OperatorReplyInsert {
                journal_id: None,
                report_date: "2026-05-28",
                reply_date: "2026-05-28",
                asset: Some("BTC"),
                decision_type: "add",
                response_class: "yes",
                conviction_implied: Some("medium"),
                timeframe_horizon: Some("weeks"),
                reasoning_summary: Some("Rotation favorable"),
                raw_content: "Adding 5% BTC on dip",
            },
        )
        .unwrap();
        assert!(id > 0);
        let rows = list(&conn, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].response_class, "yes");
    }

    #[test]
    fn list_filters_by_report_date_asset_decision() {
        let conn = fresh_conn();
        for (date, asset, decision) in [
            ("2026-05-01", "BTC", "add"),
            ("2026-05-28", "BTC", "trim"),
            ("2026-05-28", "GOLD", "hold"),
        ] {
            insert(
                &conn,
                &OperatorReplyInsert {
                    journal_id: None,
                    report_date: date,
                    reply_date: date,
                    asset: Some(asset),
                    decision_type: decision,
                    response_class: "yes",
                    conviction_implied: None,
                    timeframe_horizon: None,
                    reasoning_summary: None,
                    raw_content: "x",
                },
            )
            .unwrap();
        }
        assert_eq!(list(&conn, Some("2026-05-28"), None, None).unwrap().len(), 2);
        assert_eq!(list(&conn, None, Some("BTC"), None).unwrap().len(), 2);
        assert_eq!(list(&conn, None, None, Some("hold")).unwrap().len(), 1);
    }

    #[test]
    fn insert_rejects_invalid_inputs() {
        let conn = fresh_conn();
        let bad = insert(
            &conn,
            &OperatorReplyInsert {
                journal_id: None,
                report_date: "2026-05-28",
                reply_date: "2026-05-28",
                asset: None,
                decision_type: "smash",
                response_class: "yes",
                conviction_implied: None,
                timeframe_horizon: None,
                reasoning_summary: None,
                raw_content: "x",
            },
        );
        assert!(bad.is_err());
    }
}
