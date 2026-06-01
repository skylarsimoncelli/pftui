//! `prediction_falsification_rules` — falsification rules attached to
//! predictions. Each row describes a rule that, when triggered, marks the
//! prediction wrong. Mirrored from the live-DB enrichment session (June 1 2026).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FalsificationRule {
    pub id: i64,
    pub prediction_id: Option<i64>,
    pub rule_type: String,
    pub description: String,
    pub threshold_value: Option<f64>,
    pub threshold_symbol: Option<String>,
    pub time_horizon: Option<String>,
    pub auto_eligible: bool,
    pub created_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prediction_falsification_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER,
            rule_type TEXT NOT NULL,
            description TEXT NOT NULL,
            threshold_value REAL,
            threshold_symbol TEXT,
            time_horizon TEXT,
            auto_eligible INTEGER NOT NULL DEFAULT 0 CHECK(auto_eligible IN (0,1)),
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_falsification_rules_prediction
            ON prediction_falsification_rules(prediction_id);
        CREATE INDEX IF NOT EXISTS idx_falsification_rules_type
            ON prediction_falsification_rules(rule_type);",
    )?;
    Ok(())
}

pub fn list(
    conn: &Connection,
    rule_type: Option<&str>,
    auto_eligible_only: bool,
    for_prediction: Option<i64>,
) -> Result<Vec<FalsificationRule>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, prediction_id, rule_type, description, threshold_value,
                threshold_symbol, time_horizon, auto_eligible, created_at
         FROM prediction_falsification_rules WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(t) = rule_type {
        sql.push_str(" AND rule_type = ?");
        args.push(Box::new(t.to_string()));
    }
    if auto_eligible_only {
        sql.push_str(" AND auto_eligible = 1");
    }
    if let Some(p) = for_prediction {
        sql.push_str(" AND prediction_id = ?");
        args.push(Box::new(p));
    }
    sql.push_str(" ORDER BY id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(FalsificationRule {
                id: row.get(0)?,
                prediction_id: row.get(1)?,
                rule_type: row.get(2)?,
                description: row.get(3)?,
                threshold_value: row.get(4)?,
                threshold_symbol: row.get(5)?,
                time_horizon: row.get(6)?,
                auto_eligible: row.get::<_, i64>(7)? != 0,
                created_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn insert(
    conn: &Connection,
    prediction_id: Option<i64>,
    rule_type: &str,
    description: &str,
    threshold_value: Option<f64>,
    threshold_symbol: Option<&str>,
    time_horizon: Option<&str>,
    auto_eligible: bool,
) -> Result<i64> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO prediction_falsification_rules
            (prediction_id, rule_type, description, threshold_value,
             threshold_symbol, time_horizon, auto_eligible)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            prediction_id,
            rule_type,
            description,
            threshold_value,
            threshold_symbol,
            time_horizon,
            if auto_eligible { 1 } else { 0 },
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
    fn insert_then_list_filters() {
        let conn = fresh_conn();
        insert(
            &conn,
            Some(42),
            "price-threshold",
            "BTC closes below 80k for 3 sessions",
            Some(80000.0),
            Some("BTC"),
            Some("3d"),
            true,
        )
        .unwrap();
        insert(
            &conn,
            Some(42),
            "narrative",
            "Iran ceasefire formally signed",
            None,
            None,
            Some("weeks"),
            false,
        )
        .unwrap();
        insert(
            &conn,
            None,
            "price-threshold",
            "DXY above 110",
            Some(110.0),
            Some("DXY"),
            None,
            true,
        )
        .unwrap();

        assert_eq!(list(&conn, Some("price-threshold"), false, None).unwrap().len(), 2);
        assert_eq!(list(&conn, None, true, None).unwrap().len(), 2);
        assert_eq!(list(&conn, None, false, Some(42)).unwrap().len(), 2);
    }
}
