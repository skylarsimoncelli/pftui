use anyhow::{bail, Result};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;
use crate::db::transactions::{
    insert_transaction_backend, list_transactions_backend, set_paired_transaction_backend,
};
use crate::models::asset::AssetCategory;
use crate::models::transaction::{NewTransaction, Transaction, TxType};

/// Common cash currency symbols that should be recognized.
const KNOWN_CASH: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "SGD", "HKD",
    "KRW", "CNY", "INR", "BRL", "MXN", "ZAR", "PLN", "CZK", "HUF", "TRY", "THB", "TWD", "ILS",
    "AED", "SAR",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct SetCashOptions {
    pub confirm: bool,
    pub dry_run: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiscardPreview {
    pub id: i64,
    pub date: String,
    pub tx_type: String,
    pub quantity: String,
    pub price_per: String,
    pub signed_value: String,
    pub paired_tx_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetCashOutcome {
    pub status: String,
    pub symbol: String,
    pub amount: String,
    pub confirm_required: bool,
    pub dry_run: bool,
    pub discarded_count: usize,
    pub discard_total: String,
    pub discard_start_date: Option<String>,
    pub discard_end_date: Option<String>,
    pub discard_preview: Vec<DiscardPreview>,
    pub deleted_count: usize,
    pub inserted_tx_id: Option<i64>,
}

/// Delete all transactions for a given symbol.
fn delete_all_for_symbol(backend: &BackendConnection, symbol: &str) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| {
            let affected = conn.execute(
                "DELETE FROM transactions WHERE symbol = ?1",
                rusqlite::params![symbol],
            )?;
            Ok(affected)
        },
        |pool| delete_all_for_symbol_postgres(pool, symbol),
    )
}

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    amount: &str,
    options: SetCashOptions,
) -> Result<()> {
    let (symbol, amount, discard_preview) = prepare_set_cash(backend, symbol, amount)?;
    let confirm_required = confirm_required(&discard_preview, options);
    let preview_outcome = build_outcome(
        "requires_confirm",
        &symbol,
        amount,
        &discard_preview,
        options,
        confirm_required,
        0,
        None,
    );

    if options.json {
        if confirm_required {
            println!("{}", serde_json::to_string_pretty(&preview_outcome)?);
            bail!("{}", confirm_message(&preview_outcome));
        }
        let outcome = execute_prepared_set_cash(backend, &symbol, amount, discard_preview, options)?;
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }

    print_pre_action_summary(&preview_outcome);
    if confirm_required {
        bail!("{}", confirm_message(&preview_outcome));
    }

    let outcome = execute_prepared_set_cash(backend, &symbol, amount, discard_preview, options)?;
    print_post_action_summary(&outcome);
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_set_cash(
    backend: &BackendConnection,
    symbol: &str,
    amount: &str,
    options: SetCashOptions,
) -> Result<SetCashOutcome> {
    let (symbol, amount, discard_preview) = prepare_set_cash(backend, symbol, amount)?;
    if confirm_required(&discard_preview, options) {
        let outcome = build_outcome(
            "requires_confirm",
            &symbol,
            amount,
            &discard_preview,
            options,
            true,
            0,
            None,
        );
        bail!("{}", confirm_message(&outcome));
    }
    execute_prepared_set_cash(backend, &symbol, amount, discard_preview, options)
}

fn prepare_set_cash(
    backend: &BackendConnection,
    symbol: &str,
    amount: &str,
) -> Result<(String, Decimal, Vec<DiscardPreview>)> {
    let symbol = symbol.to_uppercase();

    if symbol.is_empty() {
        bail!("Currency symbol is required (e.g. USD, GBP, EUR)");
    }

    let amount: Decimal = amount.parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid amount: '{}'. Expected a number (e.g. 45000, 12500.50)",
            amount
        )
    })?;

    if amount < Decimal::ZERO {
        bail!("Amount cannot be negative. Use 0 to clear a cash position.");
    }

    let existing_transactions = list_transactions_backend(backend)?;
    let discard_preview = build_discard_preview(&existing_transactions, &symbol);
    Ok((symbol, amount, discard_preview))
}

fn execute_prepared_set_cash(
    backend: &BackendConnection,
    symbol: &str,
    amount: Decimal,
    discard_preview: Vec<DiscardPreview>,
    options: SetCashOptions,
) -> Result<SetCashOutcome> {
    // Warn if the symbol doesn't look like a known currency
    if !KNOWN_CASH.contains(&symbol) {
        eprintln!(
            "Warning: '{}' is not a recognized currency. Proceeding anyway.",
            symbol
        );
    }

    if options.dry_run {
        return Ok(build_outcome(
            "dry_run",
            symbol,
            amount,
            &discard_preview,
            options,
            false,
            0,
            None,
        ));
    }

    let paired_cash_rows: Vec<_> = discard_preview
        .iter()
        .filter(|tx| tx.paired_tx_id.is_some())
        .collect();
    if !paired_cash_rows.is_empty() {
        eprintln!(
            "Warning: replacing {} will discard {} paired cash leg(s). Prefer `portfolio transaction remove` for paired transactions.",
            symbol,
            paired_cash_rows.len()
        );
        for tx in &paired_cash_rows {
            if let Some(paired_id) = tx.paired_tx_id {
                set_paired_transaction_backend(backend, paired_id, None)?;
            }
        }
    }

    // Delete existing transactions for this cash symbol
    let deleted = delete_all_for_symbol(backend, symbol)?;

    if amount == Decimal::ZERO {
        let status = if deleted > 0 { "cleared" } else { "noop" };
        return Ok(build_outcome(
            status,
            symbol,
            amount,
            &discard_preview,
            options,
            false,
            deleted,
            None,
        ));
    }

    // Insert a single buy transaction at price 1.00
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let tx = NewTransaction {
        symbol: symbol.to_string(),
        category: AssetCategory::Cash,
        tx_type: TxType::Buy,
        quantity: amount,
        price_per: Decimal::ONE,
        currency: symbol.to_string(),
        date: today,
        notes: Some("Set via pftui set-cash".to_string()),
    };

    let id = insert_transaction_backend(backend, &tx)?;
    let status = if deleted > 0 { "updated" } else { "created" };
    Ok(build_outcome(
        status,
        symbol,
        amount,
        &discard_preview,
        options,
        false,
        deleted,
        Some(id),
    ))
}

fn build_discard_preview(txs: &[Transaction], symbol: &str) -> Vec<DiscardPreview> {
    txs.iter()
        .filter(|tx| tx.symbol == symbol)
        .map(|tx| DiscardPreview {
            id: tx.id,
            date: tx.date.clone(),
            tx_type: tx.tx_type.to_string(),
            quantity: tx.quantity.to_string(),
            price_per: tx.price_per.to_string(),
            signed_value: signed_value(tx).to_string(),
            paired_tx_id: tx.paired_tx_id,
        })
        .collect()
}

fn signed_value(tx: &Transaction) -> Decimal {
    let value = tx.quantity * tx.price_per;
    match tx.tx_type {
        TxType::Buy | TxType::TransferIn => value,
        TxType::Sell | TxType::TransferOut => -value,
    }
}

fn confirm_required(discard_preview: &[DiscardPreview], options: SetCashOptions) -> bool {
    discard_preview.len() > 1 && !options.confirm && !options.dry_run
}

fn discard_total(discard_preview: &[DiscardPreview]) -> Decimal {
    discard_preview
        .iter()
        .filter_map(|tx| tx.signed_value.parse::<Decimal>().ok())
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn build_outcome(
    status: &str,
    symbol: &str,
    amount: Decimal,
    discard_preview: &[DiscardPreview],
    options: SetCashOptions,
    confirm_required: bool,
    deleted_count: usize,
    inserted_tx_id: Option<i64>,
) -> SetCashOutcome {
    SetCashOutcome {
        status: status.to_string(),
        symbol: symbol.to_string(),
        amount: amount.to_string(),
        confirm_required,
        dry_run: options.dry_run,
        discarded_count: discard_preview.len(),
        discard_total: discard_total(discard_preview).to_string(),
        discard_start_date: discard_preview.first().map(|tx| tx.date.clone()),
        discard_end_date: discard_preview.last().map(|tx| tx.date.clone()),
        discard_preview: discard_preview.to_vec(),
        deleted_count,
        inserted_tx_id,
    }
}

fn confirm_message(outcome: &SetCashOutcome) -> String {
    let range = match (&outcome.discard_start_date, &outcome.discard_end_date) {
        (Some(start), Some(end)) if start != end => format!(" over date range {}..{}", start, end),
        (Some(date), _) => format!(" on {}", date),
        _ => String::new(),
    };
    format!(
        "Refusing to replace {} cash: would discard {} transactions totaling {} {}{}; pass --confirm to proceed or --dry-run to preview without changes.",
        outcome.symbol,
        outcome.discarded_count,
        outcome.discard_total,
        outcome.symbol,
        range
    )
}

fn print_pre_action_summary(outcome: &SetCashOutcome) {
    if outcome.discard_preview.is_empty() {
        println!("No existing {} transactions to discard.", outcome.symbol);
        return;
    }

    let range = match (&outcome.discard_start_date, &outcome.discard_end_date) {
        (Some(start), Some(end)) if start != end => format!(" over {}..{}", start, end),
        (Some(date), _) => format!(" on {}", date),
        _ => String::new(),
    };
    println!(
        "Replacing {} cash will discard {} transaction(s) totaling {} {}{}.",
        outcome.symbol, outcome.discarded_count, outcome.discard_total, outcome.symbol, range
    );
    println!("Rows to discard:");
    for tx in &outcome.discard_preview {
        println!(
            "  #{} {} {} qty {} @ {} signed {}",
            tx.id, tx.date, tx.tx_type, tx.quantity, tx.price_per, tx.signed_value
        );
    }
}

fn print_post_action_summary(outcome: &SetCashOutcome) {
    match outcome.status.as_str() {
        "dry_run" => {
            println!(
                "Dry run: would set {} cash to {}; no changes made.",
                outcome.symbol, outcome.amount
            );
        }
        "cleared" => {
            println!(
                "Cleared {} position ({} transaction(s) removed).",
                outcome.symbol, outcome.deleted_count
            );
        }
        "noop" => {
            println!("No existing {} position to clear.", outcome.symbol);
        }
        "updated" => {
            println!(
                "Updated {} cash position to {} (replaced {} transaction(s), new tx #{}).",
                outcome.symbol,
                outcome.amount,
                outcome.deleted_count,
                outcome.inserted_tx_id.unwrap_or_default()
            );
        }
        "created" => {
            println!(
                "Set {} cash position to {} (tx #{}).",
                outcome.symbol,
                outcome.amount,
                outcome.inserted_tx_id.unwrap_or_default()
            );
        }
        _ => {}
    }
}

fn delete_all_for_symbol_postgres(pool: &PgPool, symbol: &str) -> Result<usize> {
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM transactions WHERE symbol = $1")
            .bind(symbol)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::transactions::list_transactions_backend;
    use rusqlite::Connection;
    use rust_decimal_macros::dec;

    fn to_backend(conn: Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    fn set_cash(backend: &BackendConnection, symbol: &str, amount: &str) -> Result<SetCashOutcome> {
        apply_set_cash(backend, symbol, amount, SetCashOptions::default())
    }

    #[test]
    fn test_set_cash_creates_position() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        set_cash(&backend, "USD", "45000").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
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
        let backend = to_backend(conn);
        set_cash(&backend, "USD", "10000").unwrap();
        set_cash(&backend, "USD", "25000").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].quantity, dec!(25000));
    }

    #[test]
    fn test_set_cash_zero_clears() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        set_cash(&backend, "USD", "10000").unwrap();
        assert_eq!(list_transactions_backend(&backend).unwrap().len(), 1);

        set_cash(&backend, "USD", "0").unwrap();
        assert_eq!(list_transactions_backend(&backend).unwrap().len(), 0);
    }

    #[test]
    fn test_set_cash_zero_no_existing() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        // Should not error, just print a message
        set_cash(&backend, "USD", "0").unwrap();
        assert_eq!(list_transactions_backend(&backend).unwrap().len(), 0);
    }

    #[test]
    fn test_set_cash_uppercase() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        set_cash(&backend, "gbp", "5000").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs[0].symbol, "GBP");
    }

    #[test]
    fn test_set_cash_decimal_amount() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        set_cash(&backend, "EUR", "12500.50").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs[0].quantity, dec!(12500.50));
    }

    #[test]
    fn test_set_cash_negative_rejected() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = set_cash(&backend, "USD", "-100");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("negative"));
    }

    #[test]
    fn test_set_cash_invalid_amount() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = set_cash(&backend, "USD", "abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid amount"));
    }

    #[test]
    fn test_set_cash_multiple_currencies() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        set_cash(&backend, "USD", "45000").unwrap();
        set_cash(&backend, "GBP", "10000").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_set_cash_does_not_touch_other_symbols() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
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
        insert_transaction_backend(&backend, &equity_tx).unwrap();

        set_cash(&backend, "USD", "45000").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 2);
        assert!(txs.iter().any(|t| t.symbol == "AAPL"));
        assert!(txs.iter().any(|t| t.symbol == "USD"));
    }

    fn insert_cash_row(
        backend: &BackendConnection,
        tx_type: TxType,
        quantity: rust_decimal::Decimal,
        date: &str,
    ) -> i64 {
        let tx = crate::models::transaction::NewTransaction {
            symbol: "USD".to_string(),
            category: AssetCategory::Cash,
            tx_type,
            quantity,
            price_per: Decimal::ONE,
            currency: "USD".to_string(),
            date: date.to_string(),
            notes: None,
        };
        insert_transaction_backend(backend, &tx).unwrap()
    }

    #[test]
    fn test_set_cash_refuses_multiple_rows_without_confirm() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        insert_cash_row(&backend, TxType::Buy, dec!(10000), "2026-05-01");
        insert_cash_row(&backend, TxType::Sell, dec!(2500), "2026-05-15");

        let result = set_cash(&backend, "USD", "45000");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("would discard 2 transactions"));
        assert!(err.contains("--confirm"));
        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_set_cash_confirm_allows_multiple_rows() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        insert_cash_row(&backend, TxType::Buy, dec!(10000), "2026-05-01");
        insert_cash_row(&backend, TxType::Sell, dec!(2500), "2026-05-15");

        let outcome = apply_set_cash(
            &backend,
            "USD",
            "45000",
            SetCashOptions {
                confirm: true,
                ..SetCashOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcome.status, "updated");
        assert_eq!(outcome.discarded_count, 2);
        assert_eq!(outcome.deleted_count, 2);
        assert_eq!(outcome.discard_total, "7500");
        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].quantity, dec!(45000));
    }

    #[test]
    fn test_set_cash_dry_run_does_not_mutate() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        insert_cash_row(&backend, TxType::Buy, dec!(10000), "2026-05-01");
        insert_cash_row(&backend, TxType::Sell, dec!(2500), "2026-05-15");

        let outcome = apply_set_cash(
            &backend,
            "USD",
            "45000",
            SetCashOptions {
                dry_run: true,
                ..SetCashOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcome.status, "dry_run");
        assert_eq!(outcome.discarded_count, 2);
        assert_eq!(outcome.deleted_count, 0);
        assert_eq!(outcome.inserted_tx_id, None);
        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_set_cash_json_shape_includes_discard_preview() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let first_id = insert_cash_row(&backend, TxType::Buy, dec!(10000), "2026-05-01");
        insert_cash_row(&backend, TxType::Sell, dec!(2500), "2026-05-15");

        let outcome = apply_set_cash(
            &backend,
            "USD",
            "45000",
            SetCashOptions {
                dry_run: true,
                json: true,
                ..SetCashOptions::default()
            },
        )
        .unwrap();
        let json = serde_json::to_value(&outcome).unwrap();

        assert_eq!(json["status"], "dry_run");
        assert_eq!(json["symbol"], "USD");
        assert_eq!(json["discarded_count"], 2);
        assert_eq!(json["discard_total"], "7500");
        assert_eq!(json["discard_preview"][0]["id"], first_id);
        assert_eq!(json["discard_preview"][0]["date"], "2026-05-01");
    }

    #[test]
    fn test_set_cash_unpairs_discarded_cash_legs() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let asset_tx = crate::models::transaction::NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        };
        let cash_tx = crate::models::transaction::NewTransaction {
            symbol: "USD".to_string(),
            category: AssetCategory::Cash,
            tx_type: TxType::Sell,
            quantity: dec!(1500),
            price_per: Decimal::ONE,
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        };
        let asset_id = insert_transaction_backend(&backend, &asset_tx).unwrap();
        let cash_id = insert_transaction_backend(&backend, &cash_tx).unwrap();
        crate::db::transactions::set_paired_transaction_backend(&backend, asset_id, Some(cash_id))
            .unwrap();
        crate::db::transactions::set_paired_transaction_backend(&backend, cash_id, Some(asset_id))
            .unwrap();

        set_cash(&backend, "USD", "25000").unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        let asset = txs.iter().find(|tx| tx.id == asset_id).unwrap();
        assert_eq!(asset.paired_tx_id, None);
        assert!(txs.iter().any(|tx| tx.symbol == "USD"
            && tx.category == AssetCategory::Cash
            && tx.quantity == dec!(25000)));
        assert!(!txs.iter().any(|tx| tx.id == cash_id));
    }
}
