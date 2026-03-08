use anyhow::Result;
use rusqlite::{params, Connection, Row};
use rust_decimal::Decimal;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DividendEntry {
    pub id: i64,
    pub symbol: String,
    pub amount_per_share: Decimal,
    pub currency: String,
    pub ex_date: Option<String>,
    pub pay_date: String,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewDividendEntry {
    pub symbol: String,
    pub amount_per_share: Decimal,
    pub currency: String,
    pub ex_date: Option<String>,
    pub pay_date: String,
    pub notes: Option<String>,
}

impl DividendEntry {
    fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol: row.get(1)?,
            amount_per_share: row
                .get::<_, String>(2)?
                .parse::<Decimal>()
                .unwrap_or(Decimal::ZERO),
            currency: row.get(3)?,
            ex_date: row.get(4)?,
            pay_date: row.get(5)?,
            notes: row.get(6)?,
            created_at: row.get(7)?,
        })
    }
}

pub fn add(conn: &Connection, entry: &NewDividendEntry) -> Result<i64> {
    conn.execute(
        "INSERT INTO dividends (symbol, amount_per_share, currency, ex_date, pay_date, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.symbol,
            entry.amount_per_share.to_string(),
            entry.currency,
            entry.ex_date,
            entry.pay_date,
            entry.notes
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Connection, symbol: Option<&str>) -> Result<Vec<DividendEntry>> {
    let mut out = Vec::new();
    if let Some(sym) = symbol {
        let mut stmt = conn.prepare(
            "SELECT id, symbol, amount_per_share, currency, ex_date, pay_date, notes, created_at
             FROM dividends WHERE symbol = ?1 ORDER BY pay_date DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![sym.to_uppercase()], DividendEntry::from_row)?;
        for r in rows {
            out.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, symbol, amount_per_share, currency, ex_date, pay_date, notes, created_at
             FROM dividends ORDER BY pay_date DESC, id DESC",
        )?;
        let rows = stmt.query_map([], DividendEntry::from_row)?;
        for r in rows {
            out.push(r?);
        }
    }
    Ok(out)
}

pub fn remove(conn: &Connection, id: i64) -> Result<bool> {
    let n = conn.execute("DELETE FROM dividends WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rust_decimal_macros::dec;

    #[test]
    fn add_list_remove_round_trip() {
        let conn = db::open_in_memory();
        let id = add(
            &conn,
            &NewDividendEntry {
                symbol: "AAPL".to_string(),
                amount_per_share: dec!(0.24),
                currency: "USD".to_string(),
                ex_date: Some("2026-02-09".to_string()),
                pay_date: "2026-02-15".to_string(),
                notes: Some("Q1".to_string()),
            },
        )
        .unwrap();

        let all = list(&conn, None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
        assert_eq!(all[0].symbol, "AAPL");
        assert_eq!(all[0].amount_per_share, dec!(0.24));

        let by_symbol = list(&conn, Some("aapl")).unwrap();
        assert_eq!(by_symbol.len(), 1);

        assert!(remove(&conn, id).unwrap());
        assert!(list(&conn, None).unwrap().is_empty());
    }
}
