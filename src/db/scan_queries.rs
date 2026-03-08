use anyhow::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct ScanQueryRow {
    pub name: String,
    pub filter_expr: String,
    pub updated_at: String,
}

pub fn upsert_scan_query(conn: &Connection, name: &str, filter_expr: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO scan_queries (name, filter_expr, updated_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(name) DO UPDATE
         SET filter_expr = excluded.filter_expr,
             updated_at = datetime('now')",
        params![name, filter_expr],
    )?;
    Ok(())
}

pub fn get_scan_query(conn: &Connection, name: &str) -> Result<Option<ScanQueryRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, filter_expr, updated_at
         FROM scan_queries
         WHERE name = ?1",
    )?;
    let mut rows = stmt.query(params![name])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(ScanQueryRow {
            name: row.get(0)?,
            filter_expr: row.get(1)?,
            updated_at: row.get(2)?,
        }));
    }
    Ok(None)
}

pub fn list_scan_queries(conn: &Connection) -> Result<Vec<ScanQueryRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, filter_expr, updated_at
         FROM scan_queries
         ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ScanQueryRow {
            name: row.get(0)?,
            filter_expr: row.get(1)?,
            updated_at: row.get(2)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_get_and_list() {
        let conn = crate::db::open_in_memory();
        upsert_scan_query(&conn, "risk", "allocation_pct > 10").unwrap();
        upsert_scan_query(&conn, "risk", "allocation_pct > 12").unwrap();

        let row = get_scan_query(&conn, "risk").unwrap().unwrap();
        assert_eq!(row.filter_expr, "allocation_pct > 12");

        let all = list_scan_queries(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "risk");
    }
}
