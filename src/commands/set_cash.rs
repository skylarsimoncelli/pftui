use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rusqlite::Connection;

use crate::db::transactions::insert_transaction;
use crate::models::asset::AssetCategory;
use crate::models::transaction::{NewTransaction, TxType};

/// Common cash currency symbols that should be recognized.
const KNOWN_CASH: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "SEK", "NOK",
    "DKK", "SGD", "HKD", "KRW", "CNY", "INR", "BRL", "MXN", "ZAR", "PLN",
    "CZK", "HUF", "TRY", "THB", "TWD", "ILS", "AED", "SAR",
];

/// Delete all transactions for a given symbol.
fn delete_all_for_symbol(conn: &Connection, symbol: &str) -> Result<usize> {
    let affected = conn.execute(
        "DELETE FROM transactions WHERE symbol = ?1",
        rusqlite::params![symbol],
    )?;
    Ok(affected)
}

pub fn run(conn: &Connection, symbol: &str, amount: &str) -> Result<()> {
    let symbol = symbol.to_uppercase();

    if symbol.is_empty() {
        bail!("Currency symbol is required (e.g. USD, GBP, EUR)");
    }

    let amount: Decimal = amount.parse().map_err(|_| {
        anyhow::anyhow!("Invalid amount: '{}'. Expected a number (e.g. 45000, 12500.50)", amount)
    })?;

    if amount < Decimal::ZERO {
        bail!("Amount cannot be negative. Use 0 to clear a cash position.");
    }

    // Warn if the symbol doesn't look like a known currency
    if !KNOWN_CASH.contains(&symbol.as_str()) {
        eprintln!(
            "Warning: '{}' is not a recognized currency. Proceeding anyway.",
            symbol
        );
    }

    // Delete existing transactions for this cash symbol
    let deleted = delete_all_for_symbol(conn, &symbol)?;

    if amount == Decimal::ZERO {
        if deleted > 0 {
            println!("Cleared {} position ({} transaction(s) removed).", symbol, deleted);
        } else {
            println!("No existing {} position to clear.", symbol);
        }
        return Ok(());
    }

    // Insert a single buy transaction at price 1.00
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let tx = NewTransaction {
        symbol: symbol.clone(),
        category: AssetCategory::Cash,
        tx_type: TxType::Buy,
        quantity: amount,
        price_per: Decimal::ONE,
        currency: symbol.clone(),
        date: today,
        notes: Some("Set via pftui set-cash".to_string()),
    };

    let id = insert_transaction(conn, &tx)?;

    if deleted > 0 {
        println!(
            "Updated {} cash position to {} (replaced {} transaction(s), new tx #{}).",
            symbol, amount, deleted, id
        );
    } else {
        println!("Set {} cash position to {} (tx #{}).", symbol, amount, id);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::transactions::list_transactions;
    use rust_decimal_macros::dec;

    #[test]
    fn test_set_cash_creates_position() {
        let conn = open_in_memory();
        run(&conn, "USD", "45000").unwrap();

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].symbol, "USD");
        assert_eq!(txs[0].category, AssetCategory::Cash);
        assert_eq!(txs[0].tx_type, TxType::Buy);
        assert_eq!(txs[0].quantity, dec!(45000));
        assert_eq!(txs[0].price_per, Decimal::ONE);
    }

    #[test]
    fn test_set_cash_replaces_existing() {
        let conn = open_in_memory();
        run(&conn, "USD", "10000").unwrap();
        run(&conn, "USD", "25000").unwrap();

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].quantity, dec!(25000));
    }

    #[test]
    fn test_set_cash_zero_clears() {
        let conn = open_in_memory();
        run(&conn, "USD", "10000").unwrap();
        assert_eq!(list_transactions(&conn).unwrap().len(), 1);

        run(&conn, "USD", "0").unwrap();
        assert_eq!(list_transactions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_set_cash_zero_no_existing() {
        let conn = open_in_memory();
        // Should not error, just print a message
        run(&conn, "USD", "0").unwrap();
        assert_eq!(list_transactions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_set_cash_uppercase() {
        let conn = open_in_memory();
        run(&conn, "gbp", "5000").unwrap();

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs[0].symbol, "GBP");
    }

    #[test]
    fn test_set_cash_decimal_amount() {
        let conn = open_in_memory();
        run(&conn, "EUR", "12500.50").unwrap();

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs[0].quantity, dec!(12500.50));
    }

    #[test]
    fn test_set_cash_negative_rejected() {
        let conn = open_in_memory();
        let result = run(&conn, "USD", "-100");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("negative"));
    }

    #[test]
    fn test_set_cash_invalid_amount() {
        let conn = open_in_memory();
        let result = run(&conn, "USD", "abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid amount"));
    }

    #[test]
    fn test_set_cash_multiple_currencies() {
        let conn = open_in_memory();
        run(&conn, "USD", "45000").unwrap();
        run(&conn, "GBP", "10000").unwrap();

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_set_cash_does_not_touch_other_symbols() {
        let conn = open_in_memory();
        // Add a non-cash transaction
        let equity_tx = crate::models::transaction::NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        };
        insert_transaction(&conn, &equity_tx).unwrap();

        run(&conn, "USD", "45000").unwrap();

        let txs = list_transactions(&conn).unwrap();
        assert_eq!(txs.len(), 2);
        assert!(txs.iter().any(|t| t.symbol == "AAPL"));
        assert!(txs.iter().any(|t| t.symbol == "USD"));
    }
}
