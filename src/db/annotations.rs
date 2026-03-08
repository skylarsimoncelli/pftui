use anyhow::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct Annotation {
    pub symbol: String,
    pub thesis: String,
    pub invalidation: Option<String>,
    pub review_date: Option<String>,
    pub target_price: Option<String>,
    pub updated_at: String,
}

pub fn get_annotation(conn: &Connection, symbol: &str) -> Result<Option<Annotation>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, thesis, invalidation, review_date, target_price, updated_at
         FROM annotations
         WHERE symbol = ?1",
    )?;
    let item = stmt
        .query_row(params![symbol], |row| {
            Ok(Annotation {
                symbol: row.get(0)?,
                thesis: row.get(1)?,
                invalidation: row.get(2)?,
                review_date: row.get(3)?,
                target_price: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .ok();
    Ok(item)
}

pub fn list_annotations(conn: &Connection) -> Result<Vec<Annotation>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, thesis, invalidation, review_date, target_price, updated_at
         FROM annotations
         ORDER BY COALESCE(review_date, '9999-12-31') ASC, symbol ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Annotation {
            symbol: row.get(0)?,
            thesis: row.get(1)?,
            invalidation: row.get(2)?,
            review_date: row.get(3)?,
            target_price: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn upsert_annotation(conn: &Connection, ann: &Annotation) -> Result<()> {
    conn.execute(
        "INSERT INTO annotations (symbol, thesis, invalidation, review_date, target_price, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(symbol) DO UPDATE SET
            thesis = excluded.thesis,
            invalidation = excluded.invalidation,
            review_date = excluded.review_date,
            target_price = excluded.target_price,
            updated_at = datetime('now')",
        params![
            ann.symbol,
            ann.thesis,
            ann.invalidation,
            ann.review_date,
            ann.target_price
        ],
    )?;
    Ok(())
}

pub fn remove_annotation(conn: &Connection, symbol: &str) -> Result<bool> {
    let changed = conn.execute("DELETE FROM annotations WHERE symbol = ?1", params![symbol])?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_get_remove_roundtrip() {
        let conn = crate::db::open_in_memory();
        let ann = Annotation {
            symbol: "GC=F".to_string(),
            thesis: "Long-term inflation hedge".to_string(),
            invalidation: Some("Real rates break higher".to_string()),
            review_date: Some("2026-06-30".to_string()),
            target_price: Some("5500".to_string()),
            updated_at: String::new(),
        };
        upsert_annotation(&conn, &ann).unwrap();

        let fetched = get_annotation(&conn, "GC=F").unwrap().unwrap();
        assert_eq!(fetched.symbol, "GC=F");
        assert_eq!(fetched.thesis, "Long-term inflation hedge");
        assert_eq!(fetched.review_date.as_deref(), Some("2026-06-30"));

        let removed = remove_annotation(&conn, "GC=F").unwrap();
        assert!(removed);
        assert!(get_annotation(&conn, "GC=F").unwrap().is_none());
    }
}
