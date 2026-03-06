use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rusqlite::Connection;
use std::io::{self, Write};

use crate::db::transactions::insert_transaction;
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
    conn: &Connection,
    symbol: Option<String>,
    category: Option<String>,
    tx_type: Option<String>,
    quantity: Option<String>,
    price: Option<String>,
    currency: String,
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
    if price_per < Decimal::ZERO {
        bail!("Price per unit cannot be negative (got {})", price_per);
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
            if n.is_empty() { None } else { Some(n) }
        }
    };

    let tx = NewTransaction {
        symbol: symbol.clone(),
        category,
        tx_type,
        quantity,
        price_per,
        currency,
        date,
        notes,
    };

    let id = insert_transaction(conn, &tx)?;
    println!("Added transaction #{}: {} {} {} @ {}", id, tx_type, quantity, symbol, price_per);
    Ok(())
}
