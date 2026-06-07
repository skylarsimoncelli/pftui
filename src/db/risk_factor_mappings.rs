//! `risk_factor_mappings` — per-held-asset exposure to named risk factors.
//!
//! Drives the report's `Risk Concentration` section. Each row says:
//! "<symbol> has <exposure_multiplier> exposure (<direction>) to the
//! <factor> risk factor", e.g. "SI=F has 1.4x long exposure to
//! electrification/AI-power-demand". The macro / high analyst routines
//! write these so concentration risk can be surfaced without recomputing
//! exposure from raw correlations each run.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, PartialEq)]
pub struct RiskFactorMapping {
    pub id: i64,
    pub symbol: String,
    pub factor: String,
    pub direction: String,
    pub exposure_multiplier: f64,
    pub notes: Option<String>,
    pub created_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    crate::db::schema::run_migrations(conn)?;
    Ok(())
}

/// Upsert a single mapping. Keyed on (symbol, factor) so re-running the
/// macro routine refreshes rather than duplicating.
pub fn upsert(
    conn: &Connection,
    symbol: &str,
    factor: &str,
    direction: &str,
    exposure_multiplier: f64,
    notes: Option<&str>,
) -> Result<i64> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO risk_factor_mappings (symbol, factor, direction, exposure_multiplier, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(symbol, factor) DO UPDATE SET
            direction = excluded.direction,
            exposure_multiplier = excluded.exposure_multiplier,
            notes = excluded.notes,
            created_at = datetime('now')",
        params![
            symbol.to_uppercase(),
            factor,
            direction,
            exposure_multiplier,
            notes,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn upsert_backend(
    backend: &BackendConnection,
    symbol: &str,
    factor: &str,
    direction: &str,
    exposure_multiplier: f64,
    notes: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| upsert(conn, symbol, factor, direction, exposure_multiplier, notes),
        |_pool| anyhow::bail!("risk_factor_mappings postgres backend not yet implemented"),
    )
}

/// List all mappings, optionally filtered to a single symbol. Returns
/// rows sorted by `(symbol, factor)` for deterministic output.
pub fn list(conn: &Connection, symbol: Option<&str>) -> Result<Vec<RiskFactorMapping>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, symbol, factor, direction, exposure_multiplier, notes, created_at
         FROM risk_factor_mappings",
    );
    let upper = symbol.map(|s| s.to_uppercase());
    if upper.is_some() {
        sql.push_str(" WHERE symbol = ?");
    }
    sql.push_str(" ORDER BY symbol, factor");

    let mut stmt = conn.prepare(&sql)?;
    let mapped = |row: &rusqlite::Row<'_>| {
        Ok(RiskFactorMapping {
            id: row.get(0)?,
            symbol: row.get(1)?,
            factor: row.get(2)?,
            direction: row.get(3)?,
            exposure_multiplier: row.get(4)?,
            notes: row.get(5).ok(),
            created_at: row.get(6)?,
        })
    };
    let rows = match upper {
        Some(s) => stmt
            .query_map(params![s], mapped)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt
            .query_map([], mapped)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

pub fn list_backend(
    backend: &BackendConnection,
    symbol: Option<&str>,
) -> Result<Vec<RiskFactorMapping>> {
    query::dispatch(
        backend,
        |conn| list(conn, symbol),
        |_pool| anyhow::bail!("risk_factor_mappings postgres backend not yet implemented"),
    )
}

pub fn delete(conn: &Connection, symbol: &str, factor: &str) -> Result<usize> {
    ensure_table(conn)?;
    let n = conn.execute(
        "DELETE FROM risk_factor_mappings WHERE symbol = ?1 AND factor = ?2",
        params![symbol.to_uppercase(), factor],
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn upsert_and_list_round_trip() {
        let conn = fresh_conn();
        upsert(&conn, "SI=F", "electrification", "long", 1.4, Some("ai/grid")).unwrap();
        upsert(&conn, "GC=F", "de-dollarisation", "long", 1.2, None).unwrap();
        let rows = list(&conn, None).unwrap();
        assert_eq!(rows.len(), 2);
        // (symbol, factor) ordering puts GC=F before SI=F.
        assert_eq!(rows[0].symbol, "GC=F");
        assert_eq!(rows[1].symbol, "SI=F");
    }

    #[test]
    fn upsert_updates_existing_row() {
        let conn = fresh_conn();
        upsert(&conn, "SI=F", "electrification", "long", 1.4, None).unwrap();
        upsert(&conn, "SI=F", "electrification", "long", 1.6, Some("re-rated")).unwrap();
        let rows = list(&conn, Some("SI=F")).unwrap();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].exposure_multiplier - 1.6).abs() < 1e-9);
        assert_eq!(rows[0].notes.as_deref(), Some("re-rated"));
    }

    #[test]
    fn delete_removes_the_row() {
        let conn = fresh_conn();
        upsert(&conn, "BTC", "fixed-supply-money", "long", 1.0, None).unwrap();
        let removed = delete(&conn, "BTC", "fixed-supply-money").unwrap();
        assert_eq!(removed, 1);
        assert!(list(&conn, None).unwrap().is_empty());
    }
}
