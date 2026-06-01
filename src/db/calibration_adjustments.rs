//! `calibration_adjustments` — per-(layer, topic, conviction) computed
//! discount/boost factors derived from the calibration matrix. Mirrored from
//! the live-DB enrichment session (June 1 2026).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationAdjustment {
    pub layer: String,
    pub topic: String,
    pub conviction: String,
    pub n_scored: i64,
    pub raw_hit_rate: f64,
    pub avg_confidence: f64,
    pub adjustment_pp: f64,
    pub adjustment_direction: String,
    pub confidence_floor: Option<f64>,
    pub confidence_ceiling: Option<f64>,
    pub apply_note: String,
    pub n_threshold_met: i64,
    pub computed_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS calibration_adjustments (
            layer TEXT NOT NULL,
            topic TEXT NOT NULL,
            conviction TEXT NOT NULL,
            n_scored INTEGER NOT NULL,
            raw_hit_rate REAL NOT NULL,
            avg_confidence REAL NOT NULL,
            adjustment_pp REAL NOT NULL,
            adjustment_direction TEXT NOT NULL
                CHECK(adjustment_direction IN ('discount','boost','none')),
            confidence_floor REAL,
            confidence_ceiling REAL,
            apply_note TEXT NOT NULL,
            n_threshold_met INTEGER NOT NULL DEFAULT 1,
            computed_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(layer, topic, conviction)
        );
        CREATE INDEX IF NOT EXISTS idx_calibration_adjustments_layer
            ON calibration_adjustments(layer);
        CREATE INDEX IF NOT EXISTS idx_calibration_adjustments_topic
            ON calibration_adjustments(topic);",
    )?;
    Ok(())
}

pub fn list(
    conn: &Connection,
    layer: Option<&str>,
    topic: Option<&str>,
    conviction: Option<&str>,
) -> Result<Vec<CalibrationAdjustment>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT layer, topic, conviction, n_scored, raw_hit_rate, avg_confidence,
                adjustment_pp, adjustment_direction, confidence_floor,
                confidence_ceiling, apply_note, n_threshold_met, computed_at
         FROM calibration_adjustments WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(v) = layer {
        sql.push_str(" AND layer = ?");
        args.push(Box::new(v.to_string()));
    }
    if let Some(v) = topic {
        sql.push_str(" AND topic = ?");
        args.push(Box::new(v.to_string()));
    }
    if let Some(v) = conviction {
        sql.push_str(" AND conviction = ?");
        args.push(Box::new(v.to_string()));
    }
    sql.push_str(" ORDER BY layer ASC, topic ASC, conviction ASC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(CalibrationAdjustment {
                layer: row.get(0)?,
                topic: row.get(1)?,
                conviction: row.get(2)?,
                n_scored: row.get(3)?,
                raw_hit_rate: row.get(4)?,
                avg_confidence: row.get(5)?,
                adjustment_pp: row.get(6)?,
                adjustment_direction: row.get(7)?,
                confidence_floor: row.get(8)?,
                confidence_ceiling: row.get(9)?,
                apply_note: row.get(10)?,
                n_threshold_met: row.get(11)?,
                computed_at: row.get(12)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn upsert(
    conn: &Connection,
    layer: &str,
    topic: &str,
    conviction: &str,
    n_scored: i64,
    raw_hit_rate: f64,
    avg_confidence: f64,
    adjustment_pp: f64,
    adjustment_direction: &str,
    apply_note: &str,
) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO calibration_adjustments
            (layer, topic, conviction, n_scored, raw_hit_rate, avg_confidence,
             adjustment_pp, adjustment_direction, apply_note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(layer, topic, conviction) DO UPDATE SET
            n_scored = excluded.n_scored,
            raw_hit_rate = excluded.raw_hit_rate,
            avg_confidence = excluded.avg_confidence,
            adjustment_pp = excluded.adjustment_pp,
            adjustment_direction = excluded.adjustment_direction,
            apply_note = excluded.apply_note,
            computed_at = datetime('now')",
        params![
            layer,
            topic,
            conviction,
            n_scored,
            raw_hit_rate,
            avg_confidence,
            adjustment_pp,
            adjustment_direction,
            apply_note,
        ],
    )?;
    Ok(())
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
    fn upsert_then_list_filters() {
        let conn = fresh_conn();
        upsert(
            &conn,
            "low",
            "gold",
            "high",
            12,
            0.55,
            0.72,
            -17.0,
            "discount",
            "Discount confidence by 17pp before publishing",
        )
        .unwrap();
        upsert(
            &conn,
            "low",
            "btc",
            "medium",
            8,
            0.62,
            0.60,
            2.0,
            "boost",
            "Mild boost",
        )
        .unwrap();
        let layer_low = list(&conn, Some("low"), None, None).unwrap();
        assert_eq!(layer_low.len(), 2);
        let by_topic = list(&conn, None, Some("gold"), None).unwrap();
        assert_eq!(by_topic.len(), 1);
        assert_eq!(by_topic[0].adjustment_direction, "discount");
        let high = list(&conn, None, None, Some("high")).unwrap();
        assert_eq!(high.len(), 1);
    }
}
