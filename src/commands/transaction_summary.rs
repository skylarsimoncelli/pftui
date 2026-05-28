use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::db::allocation_targets::get_target_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::compute_positions;
use crate::models::transaction::{NewTransaction, Transaction, TxType};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TransactionChangeSummary {
    pub symbol: String,
    pub prev_alloc_pct: Option<f64>,
    pub new_alloc_pct: Option<f64>,
    pub prev_drift_pp: Option<f64>,
    pub new_drift_pp: Option<f64>,
    pub target_pct: Option<f64>,
    pub cash_delta: Option<f64>,
    pub cash_currency: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlannedCashLeg {
    pub symbol: String,
    pub tx_type: TxType,
    pub quantity: Decimal,
}

pub fn add_summary(
    backend: &BackendConnection,
    asset_tx: &NewTransaction,
    cash_leg: Option<&PlannedCashLeg>,
) -> Result<TransactionChangeSummary> {
    let before = list_transactions_backend(backend)?;
    let mut after = before.clone();
    after.push(transaction_from_new(-1, asset_tx));
    if let Some(cash_leg) = cash_leg {
        after.push(Transaction {
            id: -2,
            symbol: cash_leg.symbol.clone(),
            category: AssetCategory::Cash,
            tx_type: cash_leg.tx_type,
            quantity: cash_leg.quantity,
            price_per: Decimal::ONE,
            currency: cash_leg.symbol.clone(),
            date: asset_tx.date.clone(),
            notes: None,
            paired_tx_id: Some(-1),
            created_at: "dry-run".to_string(),
        });
    }

    let cash_delta = cash_leg.map(|leg| signed_effect(leg.tx_type, leg.quantity));
    allocation_summary(
        backend,
        &before,
        &after,
        &asset_tx.symbol,
        cash_delta,
        cash_leg.map(|leg| leg.symbol.clone()),
    )
}

pub fn remove_summary(
    backend: &BackendConnection,
    remove_ids: &[i64],
    focus_symbol: &str,
    cash_delta: Option<Decimal>,
    cash_currency: Option<String>,
) -> Result<TransactionChangeSummary> {
    let before = list_transactions_backend(backend)?;
    let remove_ids: HashSet<i64> = remove_ids.iter().copied().collect();
    let after: Vec<Transaction> = before
        .iter()
        .filter(|tx| !remove_ids.contains(&tx.id))
        .cloned()
        .collect();

    allocation_summary(
        backend,
        &before,
        &after,
        focus_symbol,
        cash_delta,
        cash_currency,
    )
}

pub fn signed_effect(tx_type: TxType, quantity: Decimal) -> Decimal {
    match tx_type {
        TxType::Buy => quantity,
        TxType::Sell => -quantity,
    }
}

pub fn removed_cash_delta(transactions: &[Transaction]) -> (Option<Decimal>, Option<String>) {
    let mut total = dec!(0);
    let mut currency = None;

    for tx in transactions {
        if tx.category != AssetCategory::Cash {
            continue;
        }
        total -= signed_effect(tx.tx_type, tx.quantity);
        currency.get_or_insert_with(|| tx.symbol.clone());
    }

    if total == dec!(0) {
        (None, currency)
    } else {
        (Some(total), currency)
    }
}

pub fn print_summary(label: &str, summary: &TransactionChangeSummary) {
    println!("{}", label);
    println!(
        "  {} allocation: {} -> {}",
        summary.symbol,
        fmt_optional_pct(summary.prev_alloc_pct),
        fmt_optional_pct(summary.new_alloc_pct)
    );

    match (
        summary.target_pct,
        summary.prev_drift_pp,
        summary.new_drift_pp,
    ) {
        (Some(target), Some(prev), Some(new)) => println!(
            "  Drift vs target {}: {} -> {}",
            fmt_pct(target),
            fmt_pct(prev),
            fmt_pct(new)
        ),
        _ => println!("  Drift vs target: N/A"),
    }

    if let Some(cash_delta) = summary.cash_delta {
        let currency = summary.cash_currency.as_deref().unwrap_or("");
        println!(
            "  Cash delta: {} {}",
            fmt_signed_number(cash_delta),
            currency
        );
    }
}

fn allocation_summary(
    backend: &BackendConnection,
    before: &[Transaction],
    after: &[Transaction],
    focus_symbol: &str,
    cash_delta: Option<Decimal>,
    cash_currency: Option<String>,
) -> Result<TransactionChangeSummary> {
    let prices = load_prices(
        before,
        after,
        cash_currency.as_deref(),
        focus_symbol,
        backend,
    )?;
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let before_positions = compute_positions(before, &prices, &fx_rates);
    let after_positions = compute_positions(after, &prices, &fx_rates);

    let target = get_target_backend(backend, focus_symbol)?;
    let target_pct = target.as_ref().map(|target| target.target_pct);
    let prev_alloc = allocation_pct(&before_positions, focus_symbol);
    let new_alloc = allocation_pct(&after_positions, focus_symbol);
    let prev_drift = target_pct.and_then(|target| prev_alloc.map(|alloc| alloc - target));
    let new_drift = target_pct.and_then(|target| new_alloc.map(|alloc| alloc - target));

    Ok(TransactionChangeSummary {
        symbol: focus_symbol.to_string(),
        prev_alloc_pct: prev_alloc.map(|value| decimal_to_f64(value, 2)),
        new_alloc_pct: new_alloc.map(|value| decimal_to_f64(value, 2)),
        prev_drift_pp: prev_drift.map(|value| decimal_to_f64(value, 2)),
        new_drift_pp: new_drift.map(|value| decimal_to_f64(value, 2)),
        target_pct: target_pct.map(|value| decimal_to_f64(value, 2)),
        cash_delta: cash_delta.map(|value| decimal_to_f64(value, 2)),
        cash_currency,
    })
}

fn load_prices(
    before: &[Transaction],
    after: &[Transaction],
    cash_currency: Option<&str>,
    focus_symbol: &str,
    backend: &BackendConnection,
) -> Result<HashMap<String, Decimal>> {
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect();

    for tx in before.iter().chain(after.iter()) {
        if tx.category == AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }
    if let Some(currency) = cash_currency {
        prices.insert(currency.to_string(), Decimal::ONE);
    }
    if focus_symbol == cash_currency.unwrap_or_default() {
        prices.insert(focus_symbol.to_string(), Decimal::ONE);
    }

    Ok(prices)
}

fn allocation_pct(
    positions: &[crate::models::position::Position],
    symbol: &str,
) -> Option<Decimal> {
    match positions.iter().find(|position| position.symbol == symbol) {
        Some(position) => position.allocation_pct,
        None => Some(dec!(0)),
    }
}

fn transaction_from_new(id: i64, tx: &NewTransaction) -> Transaction {
    Transaction {
        id,
        symbol: tx.symbol.clone(),
        category: tx.category,
        tx_type: tx.tx_type,
        quantity: tx.quantity,
        price_per: tx.price_per,
        currency: tx.currency.clone(),
        date: tx.date.clone(),
        notes: tx.notes.clone(),
        paired_tx_id: None,
        created_at: "dry-run".to_string(),
    }
}

fn decimal_to_f64(value: Decimal, dp: u32) -> f64 {
    value.round_dp(dp).to_string().parse::<f64>().unwrap_or(0.0)
}

fn fmt_optional_pct(value: Option<f64>) -> String {
    value.map(fmt_pct).unwrap_or_else(|| "N/A".to_string())
}

fn fmt_pct(value: f64) -> String {
    if value.abs() < 0.005 {
        "0.00%".to_string()
    } else {
        format!("{value:+.2}%")
    }
}

fn fmt_signed_number(value: f64) -> String {
    if value.abs() < 0.005 {
        "0.00".to_string()
    } else {
        format!("{value:+.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::allocation_targets::set_target_backend;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::price_cache::upsert_price_backend;
    use crate::db::transactions::insert_transaction_backend;
    use crate::models::price::PriceQuote;
    use rust_decimal_macros::dec;

    fn backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: open_in_memory(),
        }
    }

    fn tx(
        symbol: &str,
        category: AssetCategory,
        tx_type: TxType,
        quantity: Decimal,
    ) -> NewTransaction {
        NewTransaction {
            symbol: symbol.to_string(),
            category,
            tx_type,
            quantity,
            price_per: dec!(100),
            currency: "USD".to_string(),
            date: "2026-05-28".to_string(),
            notes: None,
        }
    }

    fn price(backend: &BackendConnection, symbol: &str, value: Decimal) {
        upsert_price_backend(
            backend,
            &PriceQuote {
                symbol: symbol.to_string(),
                price: value,
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2026-05-28T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn add_summary_reports_allocation_and_drift_delta() {
        let backend = backend();
        price(&backend, "AAPL", dec!(100));
        price(&backend, "GC=F", dec!(100));
        insert_transaction_backend(
            &backend,
            &tx("AAPL", AssetCategory::Equity, TxType::Buy, dec!(10)),
        )
        .unwrap();
        set_target_backend(&backend, "GC=F", dec!(50), dec!(2)).unwrap();

        let asset_tx = tx("GC=F", AssetCategory::Commodity, TxType::Buy, dec!(10));
        let cash_leg = PlannedCashLeg {
            symbol: "USD".to_string(),
            tx_type: TxType::Sell,
            quantity: dec!(1000),
        };

        let summary = add_summary(&backend, &asset_tx, Some(&cash_leg)).unwrap();

        assert_eq!(summary.symbol, "GC=F");
        assert_eq!(summary.prev_alloc_pct, Some(0.0));
        assert_eq!(summary.new_alloc_pct, Some(50.0));
        assert_eq!(summary.prev_drift_pp, Some(-50.0));
        assert_eq!(summary.new_drift_pp, Some(0.0));
        assert_eq!(summary.target_pct, Some(50.0));
        assert_eq!(summary.cash_delta, Some(-1000.0));
        assert_eq!(summary.cash_currency.as_deref(), Some("USD"));
    }

    #[test]
    fn remove_summary_reports_after_state_without_mutating() {
        let backend = backend();
        price(&backend, "AAPL", dec!(100));
        price(&backend, "GC=F", dec!(100));
        let aapl_id = insert_transaction_backend(
            &backend,
            &tx("AAPL", AssetCategory::Equity, TxType::Buy, dec!(10)),
        )
        .unwrap();
        insert_transaction_backend(
            &backend,
            &tx("GC=F", AssetCategory::Commodity, TxType::Buy, dec!(10)),
        )
        .unwrap();

        let summary = remove_summary(&backend, &[aapl_id], "AAPL", None, None).unwrap();

        assert_eq!(summary.prev_alloc_pct, Some(50.0));
        assert_eq!(summary.new_alloc_pct, Some(0.0));
    }
}
