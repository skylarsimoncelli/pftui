//! `event_annotations` — operator-curated catalogue of macro/market events
//! used to annotate prediction outcomes. Mirrors the live-DB enrichment
//! session (June 1 2026).

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const VALID_CATEGORIES: &[&str] = &[
    "geopolitical",
    "monetary-policy",
    "economic-data",
    "market-structure",
    "crisis",
    "operator-action",
    "catalyst-other",
];

pub const VALID_PERSISTENCE: &[&str] = &["transient", "days", "weeks", "structural"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventAnnotation {
    pub id: i64,
    pub event_date: String,
    pub event_time: Option<String>,
    pub category: String,
    pub asset_impact: Vec<String>,
    pub headline: String,
    pub detail: Option<String>,
    pub source: Option<String>,
    pub magnitude: i64,
    pub persistence: Option<String>,
    pub related_predictions: Vec<i64>,
    pub related_scenarios: Vec<String>,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct EventAnnotationInsert<'a> {
    pub event_date: &'a str,
    pub event_time: Option<&'a str>,
    pub category: &'a str,
    pub headline: &'a str,
    pub detail: Option<&'a str>,
    pub source: Option<&'a str>,
    pub magnitude: i64,
    pub persistence: Option<&'a str>,
    pub asset_impact: &'a [String],
    pub related_predictions: &'a [i64],
    pub related_scenarios: &'a [String],
    pub notes: Option<&'a str>,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS event_annotations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_date TEXT NOT NULL,
            event_time TEXT,
            category TEXT NOT NULL CHECK(category IN (
                'geopolitical','monetary-policy','economic-data',
                'market-structure','crisis','operator-action','catalyst-other'
            )),
            asset_impact TEXT NOT NULL DEFAULT '[]',
            headline TEXT NOT NULL,
            detail TEXT,
            source TEXT,
            magnitude INTEGER NOT NULL DEFAULT 3 CHECK(magnitude BETWEEN 1 AND 5),
            persistence TEXT CHECK(
                persistence IN ('transient','days','weeks','structural')
                OR persistence IS NULL
            ),
            related_predictions TEXT NOT NULL DEFAULT '[]',
            related_scenarios TEXT NOT NULL DEFAULT '[]',
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_event_annotations_date
            ON event_annotations(event_date);
        CREATE INDEX IF NOT EXISTS idx_event_annotations_category
            ON event_annotations(category);",
    )?;
    Ok(())
}

fn parse_string_array(raw: Option<String>) -> Vec<String> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

fn parse_i64_array(raw: Option<String>) -> Vec<i64> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<i64>>(s).ok())
        .unwrap_or_default()
}

fn json_strings(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

fn json_i64s(values: &[i64]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

pub fn list(
    conn: &Connection,
    category: Option<&str>,
    since: Option<&str>,
    asset: Option<&str>,
) -> Result<Vec<EventAnnotation>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, event_date, event_time, category, asset_impact, headline,
                detail, source, magnitude, persistence, related_predictions,
                related_scenarios, notes, created_at
         FROM event_annotations WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(c) = category {
        sql.push_str(" AND category = ?");
        args.push(Box::new(c.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND event_date >= ?");
        args.push(Box::new(s.to_string()));
    }
    sql.push_str(" ORDER BY event_date DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_slice.as_slice(), |row| {
        Ok(EventAnnotation {
            id: row.get(0)?,
            event_date: row.get(1)?,
            event_time: row.get(2)?,
            category: row.get(3)?,
            asset_impact: parse_string_array(row.get(4)?),
            headline: row.get(5)?,
            detail: row.get(6)?,
            source: row.get(7)?,
            magnitude: row.get(8)?,
            persistence: row.get(9)?,
            related_predictions: parse_i64_array(row.get(10)?),
            related_scenarios: parse_string_array(row.get(11)?),
            notes: row.get(12)?,
            created_at: row.get(13)?,
        })
    })?;
    let mut results: Vec<EventAnnotation> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if let Some(a) = asset {
        let asset_lower = a.to_lowercase();
        results.retain(|e| {
            e.asset_impact
                .iter()
                .any(|x| x.to_lowercase() == asset_lower)
        });
    }
    Ok(results)
}

pub fn insert(conn: &Connection, row: &EventAnnotationInsert<'_>) -> Result<i64> {
    ensure_table(conn)?;
    if !VALID_CATEGORIES.contains(&row.category) {
        return Err(anyhow!(
            "invalid category '{}'; must be one of {}",
            row.category,
            VALID_CATEGORIES.join("|")
        ));
    }
    if let Some(p) = row.persistence {
        if !VALID_PERSISTENCE.contains(&p) {
            return Err(anyhow!(
                "invalid persistence '{}'; must be one of {}",
                p,
                VALID_PERSISTENCE.join("|")
            ));
        }
    }
    if !(1..=5).contains(&row.magnitude) {
        return Err(anyhow!(
            "magnitude must be between 1 and 5, got {}",
            row.magnitude
        ));
    }
    conn.execute(
        "INSERT INTO event_annotations
            (event_date, event_time, category, asset_impact, headline, detail,
             source, magnitude, persistence, related_predictions,
             related_scenarios, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            row.event_date,
            row.event_time,
            row.category,
            json_strings(row.asset_impact),
            row.headline,
            row.detail,
            row.source,
            row.magnitude,
            row.persistence,
            json_i64s(row.related_predictions),
            json_strings(row.related_scenarios),
            row.notes,
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
    fn insert_then_list_roundtrips_assets_and_persistence() {
        let conn = fresh_conn();
        let assets = vec!["BTC".to_string(), "GOLD".to_string()];
        let scenarios = vec!["iran-oil-spike".to_string()];
        let id = insert(
            &conn,
            &EventAnnotationInsert {
                event_date: "2026-05-12",
                event_time: Some("14:00:00"),
                category: "geopolitical",
                headline: "Iran retaliation",
                detail: Some("Naval skirmish in strait"),
                source: None,
                magnitude: 4,
                persistence: Some("days"),
                asset_impact: &assets,
                related_predictions: &[101, 102],
                related_scenarios: &scenarios,
                notes: None,
            },
        )
        .unwrap();
        assert!(id > 0);
        let rows = list(&conn, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset_impact, assets);
        assert_eq!(rows[0].related_predictions, vec![101, 102]);
        assert_eq!(rows[0].related_scenarios, scenarios);
        assert_eq!(rows[0].persistence.as_deref(), Some("days"));
    }

    #[test]
    fn list_filters_category_since_and_asset() {
        let conn = fresh_conn();
        insert(
            &conn,
            &EventAnnotationInsert {
                event_date: "2026-04-01",
                event_time: None,
                category: "monetary-policy",
                headline: "FOMC hold",
                detail: None,
                source: None,
                magnitude: 3,
                persistence: None,
                asset_impact: &["SPY".to_string()],
                related_predictions: &[],
                related_scenarios: &[],
                notes: None,
            },
        )
        .unwrap();
        insert(
            &conn,
            &EventAnnotationInsert {
                event_date: "2026-05-15",
                event_time: None,
                category: "geopolitical",
                headline: "Strike",
                detail: None,
                source: None,
                magnitude: 4,
                persistence: None,
                asset_impact: &["BTC".to_string(), "GOLD".to_string()],
                related_predictions: &[],
                related_scenarios: &[],
                notes: None,
            },
        )
        .unwrap();

        assert_eq!(list(&conn, Some("monetary-policy"), None, None).unwrap().len(), 1);
        let since = list(&conn, None, Some("2026-05-01"), None).unwrap();
        assert_eq!(since.len(), 1);
        assert_eq!(since[0].category, "geopolitical");
        let asset = list(&conn, None, None, Some("btc")).unwrap();
        assert_eq!(asset.len(), 1);
        assert_eq!(asset[0].headline, "Strike");
    }

    #[test]
    fn insert_rejects_invalid_category_and_magnitude() {
        let conn = fresh_conn();
        let bad_cat = insert(
            &conn,
            &EventAnnotationInsert {
                event_date: "2026-04-01",
                event_time: None,
                category: "bogus",
                headline: "X",
                detail: None,
                source: None,
                magnitude: 3,
                persistence: None,
                asset_impact: &[],
                related_predictions: &[],
                related_scenarios: &[],
                notes: None,
            },
        );
        assert!(bad_cat.is_err());
        let bad_mag = insert(
            &conn,
            &EventAnnotationInsert {
                event_date: "2026-04-01",
                event_time: None,
                category: "crisis",
                headline: "X",
                detail: None,
                source: None,
                magnitude: 9,
                persistence: None,
                asset_impact: &[],
                related_predictions: &[],
                related_scenarios: &[],
                notes: None,
            },
        );
        assert!(bad_mag.is_err());
    }
}
