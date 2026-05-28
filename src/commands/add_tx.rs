use anyhow::{bail, Context, Result};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::io::{self, Write};

use crate::db::backend::BackendConnection;
use crate::db::fx_cache::get_all_fx_rates_backend;
use crate::db::transactions::{
    delete_transaction_backend, insert_transaction_backend, set_paired_transaction_backend,
};
use crate::models::asset::AssetCategory;
use crate::models::transaction::{NewTransaction, TxType};

fn prompt(label: &str) -> Result<String> {
    print!("{}: ", label);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    symbol: Option<String>,
    category: Option<String>,
    tx_type: Option<String>,
    quantity: Option<String>,
    price: Option<String>,
    currency: String,
    cash_currency: String,
    no_auto_cash: bool,
    date: Option<String>,
    notes: Option<String>,
) -> Result<()> {
    let symbol = match symbol {
        Some(s) => s.to_uppercase(),
        None => prompt("Symbol (e.g. AAPL, BTC)")?.to_uppercase(),
    };
    if symbol.is_empty() {
        bail!("Symbol is required");
    }

    let category: AssetCategory = match category {
        Some(c) => c.parse()?,
        None => prompt("Category (equity/crypto/forex/cash/commodity/fund)")?.parse()?,
    };

    let tx_type: TxType = match tx_type {
        Some(t) => t.parse()?,
        None => prompt("Type (buy/sell)")?.parse()?,
    };

    let quantity: Decimal = match quantity {
        Some(q) => q.parse()?,
        None => prompt("Quantity")?.parse()?,
    };
    if quantity <= Decimal::ZERO {
        bail!("Quantity must be greater than zero (got {})", quantity);
    }

    let price_per: Decimal = match price {
        Some(p) => p.parse()?,
        None => prompt("Price per unit")?.parse()?,
    };
    if price_per <= Decimal::ZERO {
        bail!(
            "Price per unit must be greater than zero (got {})",
            price_per
        );
    }

    let date = match date {
        Some(d) => d,
        None => prompt("Date (YYYY-MM-DD)")?,
    };

    let notes = match notes {
        Some(n) if !n.is_empty() => Some(n),
        Some(_) => None,
        None => {
            let n = prompt("Notes (optional, press Enter to skip)")?;
            if n.is_empty() {
                None
            } else {
                Some(n)
            }
        }
    };

    let currency = normalize_currency(&currency);
    let cash_currency = normalize_currency(&cash_currency);
    if currency.is_empty() {
        bail!("Currency is required");
    }
    if cash_currency.is_empty() {
        bail!("Cash currency is required");
    }
    let auto_cash = !no_auto_cash && category != AssetCategory::Cash;
    let cash_leg = if auto_cash {
        let trade_value = quantity * price_per;
        let rates = get_all_fx_rates_backend(backend)?;
        let (cash_amount, fx_rate) =
            convert_cash_amount(trade_value, &currency, &cash_currency, &rates)?;
        Some((cash_amount, fx_rate, trade_value))
    } else {
        None
    };

    let tx = NewTransaction {
        symbol: symbol.clone(),
        category,
        tx_type,
        quantity,
        price_per,
        currency: currency.clone(),
        date,
        notes,
    };

    let id = insert_transaction_backend(backend, &tx)?;
    let cash_summary = if let Some((cash_amount, fx_rate, trade_value)) = cash_leg {
        let cash_tx = NewTransaction {
            symbol: cash_currency.clone(),
            category: AssetCategory::Cash,
            tx_type: cash_leg_type(tx_type),
            quantity: cash_amount,
            price_per: Decimal::ONE,
            currency: cash_currency.clone(),
            date: tx.date.clone(),
            notes: Some(cash_leg_notes(
                id,
                trade_value,
                &currency,
                &cash_currency,
                fx_rate,
            )),
        };

        match insert_transaction_backend(backend, &cash_tx) {
            Ok(cash_id) => {
                if let Err(e) = set_paired_transaction_backend(backend, id, Some(cash_id))
                    .and_then(|_| set_paired_transaction_backend(backend, cash_id, Some(id)))
                {
                    let _ = delete_transaction_backend(backend, cash_id);
                    let _ = delete_transaction_backend(backend, id);
                    return Err(e).context("failed to link paired cash transaction");
                }
                Some((cash_id, cash_amount))
            }
            Err(e) => {
                let _ = delete_transaction_backend(backend, id);
                return Err(e).context("failed to insert paired cash transaction");
            }
        }
    } else {
        None
    };

    println!(
        "Added transaction #{}: {} {} {} @ {}",
        id, tx_type, quantity, symbol, price_per
    );
    if let Some((cash_id, cash_amount)) = cash_summary {
        println!(
            "Added paired cash transaction #{}: {} {} {} @ 1",
            cash_id,
            cash_leg_type(tx_type),
            cash_amount,
            cash_currency
        );
    }
    Ok(())
}

fn normalize_currency(currency: &str) -> String {
    currency.trim().to_uppercase()
}

fn cash_leg_type(tx_type: TxType) -> TxType {
    match tx_type {
        TxType::Buy => TxType::Sell,
        TxType::Sell => TxType::Buy,
    }
}

fn rate_to_usd(currency: &str, rates: &HashMap<String, Decimal>) -> Result<Decimal> {
    if currency == "USD" {
        return Ok(Decimal::ONE);
    }
    rates.get(currency).copied().ok_or_else(|| {
        anyhow::anyhow!(
            "No fresh FX rate for {}. Run `pftui data refresh --only fx-rates` or pass --no-auto-cash.",
            currency
        )
    })
}

fn convert_cash_amount(
    trade_value: Decimal,
    price_currency: &str,
    cash_currency: &str,
    rates: &HashMap<String, Decimal>,
) -> Result<(Decimal, Option<Decimal>)> {
    if price_currency == cash_currency {
        return Ok((trade_value, None));
    }

    let price_to_usd = rate_to_usd(price_currency, rates)?;
    let cash_to_usd = rate_to_usd(cash_currency, rates)?;
    if cash_to_usd <= Decimal::ZERO {
        bail!("Invalid FX rate for {}", cash_currency);
    }
    let price_to_cash = price_to_usd / cash_to_usd;
    Ok((trade_value * price_to_cash, Some(price_to_cash)))
}

fn cash_leg_notes(
    asset_tx_id: i64,
    trade_value: Decimal,
    price_currency: &str,
    cash_currency: &str,
    fx_rate: Option<Decimal>,
) -> String {
    match fx_rate {
        Some(rate) => format!(
            "Auto cash leg paired with tx #{}; original value {} {}; fx {}->{} {}",
            asset_tx_id,
            trade_value,
            price_currency,
            price_currency,
            cash_currency,
            rate.round_dp(6)
        ),
        None => format!("Auto cash leg paired with tx #{}", asset_tx_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::fx_cache::upsert_fx_rate_backend;
    use crate::db::open_in_memory;
    use crate::db::transactions::list_transactions_backend;
    use rust_decimal_macros::dec;

    fn backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: open_in_memory(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        backend: &BackendConnection,
        symbol: &str,
        category: &str,
        tx_type: &str,
        quantity: &str,
        price: &str,
        currency: &str,
        cash_currency: &str,
        no_auto_cash: bool,
    ) -> Result<()> {
        run(
            backend,
            Some(symbol.to_string()),
            Some(category.to_string()),
            Some(tx_type.to_string()),
            Some(quantity.to_string()),
            Some(price.to_string()),
            currency.to_string(),
            cash_currency.to_string(),
            no_auto_cash,
            Some("2026-05-28".to_string()),
            Some("test".to_string()),
        )
    }

    #[test]
    fn buy_inserts_paired_cash_debit() {
        let backend = backend();
        add(
            &backend,
            "GC=F",
            "commodity",
            "buy",
            "2",
            "4500",
            "USD",
            "USD",
            false,
        )
        .unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 2);
        let asset = txs.iter().find(|tx| tx.symbol == "GC=F").unwrap();
        let cash = txs.iter().find(|tx| tx.symbol == "USD").unwrap();
        assert_eq!(cash.category, AssetCategory::Cash);
        assert_eq!(cash.tx_type, TxType::Sell);
        assert_eq!(cash.quantity, dec!(9000));
        assert_eq!(cash.price_per, Decimal::ONE);
        assert_eq!(asset.paired_tx_id, Some(cash.id));
        assert_eq!(cash.paired_tx_id, Some(asset.id));
        assert!(cash
            .notes
            .as_deref()
            .unwrap_or_default()
            .contains(&format!("tx #{}", asset.id)));
    }

    #[test]
    fn sell_inserts_paired_cash_credit() {
        let backend = backend();
        add(
            &backend, "AAPL", "equity", "sell", "3", "200", "USD", "USD", false,
        )
        .unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        let cash = txs.iter().find(|tx| tx.symbol == "USD").unwrap();
        assert_eq!(cash.tx_type, TxType::Buy);
        assert_eq!(cash.quantity, dec!(600));
    }

    #[test]
    fn no_auto_cash_inserts_only_asset_leg() {
        let backend = backend();
        add(
            &backend, "AAPL", "equity", "buy", "1", "100", "USD", "USD", true,
        )
        .unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].symbol, "AAPL");
        assert_eq!(txs[0].paired_tx_id, None);
    }

    #[test]
    fn non_usd_price_uses_fx_cache_for_cash_leg() {
        let backend = backend();
        upsert_fx_rate_backend(&backend, "EUR", dec!(1.10)).unwrap();

        add(
            &backend, "VWRL.L", "fund", "buy", "10", "10", "EUR", "USD", false,
        )
        .unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        let cash = txs.iter().find(|tx| tx.symbol == "USD").unwrap();
        assert_eq!(cash.tx_type, TxType::Sell);
        assert_eq!(cash.quantity, dec!(110.0));
        assert!(cash
            .notes
            .as_deref()
            .unwrap_or_default()
            .contains("fx EUR->USD"));
    }
}
