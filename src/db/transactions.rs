use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};

use crate::models::asset::AssetCategory;
use crate::models::transaction::{NewTransaction, Transaction, TxType};

pub fn insert_transaction(conn: &Connection, tx: &NewTransaction) -> Result<i64> {
    conn.execute(
        "INSERT INTO transactions (symbol, category, tx_type, quantity, price_per, currency, date, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            tx.symbol,
            tx.category.to_string(),
            tx.tx_type.to_string(),
            tx.quantity.to_string(),
            tx.price_per.to_string(),
            tx.currency,
            tx.date,
            tx.notes,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_transaction(conn: &Connection, id: i64) -> Result<bool> {
    let affected = conn.execute("DELETE FROM transactions WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

pub fn update_transaction(conn: &Connection, id: i64, tx: &NewTransaction) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE transactions
         SET symbol = ?1, category = ?2, tx_type = ?3, quantity = ?4, price_per = ?5, currency = ?6, date = ?7, notes = ?8
         WHERE id = ?9",
        params![
            tx.symbol,
            tx.category.to_string(),
            tx.tx_type.to_string(),
            tx.quantity.to_string(),
            tx.price_per.to_string(),
            tx.currency,
            tx.date,
            tx.notes,
            id,
        ],
    )?;
    Ok(affected > 0)
}

pub fn list_transactions(conn: &Connection) -> Result<Vec<Transaction>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, category, tx_type, quantity, price_per, currency, date, notes, created_at
         FROM transactions ORDER BY date ASC, id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Transaction {
            id: row.get(0)?,
            symbol: row.get(1)?,
            category: row.get::<_, String>(2)?.parse().unwrap_or(AssetCategory::Equity),
            tx_type: row.get::<_, String>(3)?.parse().unwrap_or(TxType::Buy),
            quantity: row.get::<_, String>(4)?.parse().unwrap_or(Decimal::ZERO),
            price_per: row.get::<_, String>(5)?.parse().unwrap_or(Decimal::ZERO),
            currency: row.get(6)?,
            date: row.get(7)?,
            notes: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_transaction(conn: &Connection, id: i64) -> Result<Option<Transaction>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, category, tx_type, quantity, price_per, currency, date, notes, created_at
         FROM transactions WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Transaction {
            id: row.get(0)?,
            symbol: row.get(1)?,
            category: row.get::<_, String>(2)?.parse().unwrap_or(AssetCategory::Equity),
            tx_type: row.get::<_, String>(3)?.parse().unwrap_or(TxType::Buy),
            quantity: row.get::<_, String>(4)?.parse().unwrap_or(Decimal::ZERO),
            price_per: row.get::<_, String>(5)?.parse().unwrap_or(Decimal::ZERO),
            currency: row.get(6)?,
            date: row.get(7)?,
            notes: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn count_transactions(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions",
        [],
        |r| r.get(0),
    )?;
    Ok(count)
}

pub fn get_unique_symbols(conn: &Connection) -> Result<Vec<(String, AssetCategory)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT symbol, category FROM transactions ORDER BY symbol",
    )?;
    let rows = stmt.query_map([], |row| {
        let symbol: String = row.get(0)?;
        let cat: String = row.get(1)?;
        Ok((symbol, cat.parse().unwrap_or(AssetCategory::Equity)))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    fn sample_tx() -> NewTransaction {
        NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: Some("test buy".to_string()),
        }
    }

    #[test]
    fn test_insert_and_list() {
        let conn = open_in_memory();
        let id = insert_transaction(&conn, &sample_tx()).unwrap();
        assert!(id > 0);

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].symbol, "AAPL");
        assert_eq!(txs[0].quantity, dec!(10));
    }

    #[test]
    fn test_delete() {
        let conn = open_in_memory();
        let id = insert_transaction(&conn, &sample_tx()).unwrap();
        assert!(delete_transaction(&conn, id).unwrap());
        assert!(!delete_transaction(&conn, id).unwrap());
        assert_eq!(list_transactions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_update() {
        let conn = open_in_memory();
        let id = insert_transaction(&conn, &sample_tx()).unwrap();
        let mut updated = sample_tx();
        updated.symbol = "MSFT".to_string();
        updated.tx_type = TxType::Sell;
        updated.quantity = dec!(5);
        assert!(update_transaction(&conn, id, &updated).unwrap());
        let tx = get_transaction(&conn, id).unwrap().unwrap();
        assert_eq!(tx.symbol, "MSFT");
        assert_eq!(tx.tx_type, TxType::Sell);
        assert_eq!(tx.quantity, dec!(5));
    }

    #[test]
    fn test_get_unique_symbols() {
        let conn = open_in_memory();
        insert_transaction(&conn, &sample_tx()).unwrap();
        let mut tx2 = sample_tx();
        tx2.symbol = "GOOG".to_string();
        insert_transaction(&conn, &tx2).unwrap();

        let symbols = get_unique_symbols(&conn).unwrap();
        assert_eq!(symbols.len(), 2);
    }
}
