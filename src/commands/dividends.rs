use anyhow::{anyhow, bail, Result};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::db::backend::BackendConnection;
use crate::db::dividends as dividends_db;
use crate::db::price_cache;
use crate::db::transactions;
use crate::models::transaction::TxType;

pub struct DividendsArgs {
    pub value: Option<String>,
    pub amount: Option<String>,
    pub pay_date: Option<String>,
    pub ex_date: Option<String>,
    pub currency: String,
    pub notes: Option<String>,
    pub json: bool,
}

#[derive(serde::Serialize)]
struct DividendViewRow {
    id: i64,
    symbol: String,
    ex_date: Option<String>,
    pay_date: String,
    amount_per_share: String,
    shares_estimate: String,
    cash_amount_estimate: String,
    ttm_yield_pct: Option<String>,
    currency: String,
    notes: Option<String>,
}

pub fn run(backend: &BackendConnection, action: &str, args: DividendsArgs) -> Result<()> {
    match action {
        "add" => run_add(backend, &args),
        "list" => run_list(backend, &args),
        "remove" => run_remove(backend, &args),
        _ => bail!(
            "Unknown dividends action '{}'. Use: add, list, remove",
            action
        ),
    }
}

fn run_add(backend: &BackendConnection, args: &DividendsArgs) -> Result<()> {
    let symbol = args
        .value
        .as_deref()
        .ok_or_else(|| anyhow!("Usage: pftui dividends add SYMBOL --amount N --pay-date YYYY-MM-DD [--ex-date YYYY-MM-DD]"))?
        .to_uppercase();

    let amount_str = args
        .amount
        .as_deref()
        .ok_or_else(|| anyhow!("--amount is required"))?;
    let amount = parse_decimal(amount_str)?;
    if amount <= Decimal::ZERO {
        bail!("--amount must be > 0");
    }

    let pay_date = args
        .pay_date
        .as_deref()
        .ok_or_else(|| anyhow!("--pay-date is required (YYYY-MM-DD)"))?;
    validate_date(pay_date)?;
    if let Some(ex_date) = args.ex_date.as_deref() {
        validate_date(ex_date)?;
    }

    let id = dividends_db::add_backend(
        backend,
        &dividends_db::NewDividendEntry {
            symbol: symbol.clone(),
            amount_per_share: amount,
            currency: args.currency.to_uppercase(),
            ex_date: args.ex_date.clone(),
            pay_date: pay_date.to_string(),
            notes: args.notes.clone(),
        },
    )?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": id,
                "symbol": symbol,
                "amount_per_share": amount.to_string(),
                "pay_date": pay_date,
                "ex_date": args.ex_date,
                "currency": args.currency.to_uppercase(),
                "notes": args.notes,
            }))?
        );
    } else {
        println!(
            "Added dividend #{}: {} {} per share (pay date: {})",
            id, symbol, amount, pay_date
        );
    }
    Ok(())
}

fn run_list(backend: &BackendConnection, args: &DividendsArgs) -> Result<()> {
    let symbol_filter = args
        .value
        .as_deref()
        .map(|s| s.to_uppercase())
        .filter(|s| !s.is_empty());
    let rows = dividends_db::list_backend(backend, symbol_filter.as_deref())?;

    if rows.is_empty() {
        if args.json {
            println!("[]");
        } else {
            println!("No dividends found.");
        }
        return Ok(());
    }

    let share_map = current_shares_by_symbol(backend)?;
    let ttm_map = trailing_12m_div_per_share(backend)?;
    let price_map = price_cache::get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|p| (p.symbol, p.price))
        .collect::<std::collections::HashMap<_, _>>();

    let output: Vec<DividendViewRow> = rows
        .iter()
        .map(|r| {
            let shares = share_map.get(&r.symbol).copied().unwrap_or(Decimal::ZERO);
            let cash = r.amount_per_share * shares;
            let ttm_yield_pct = match (ttm_map.get(&r.symbol), price_map.get(&r.symbol)) {
                (Some(ttm_div), Some(price)) if *price > Decimal::ZERO => {
                    Some(((*ttm_div / *price) * dec!(100)).round_dp(2).to_string())
                }
                _ => None,
            };

            DividendViewRow {
                id: r.id,
                symbol: r.symbol.clone(),
                ex_date: r.ex_date.clone(),
                pay_date: r.pay_date.clone(),
                amount_per_share: r.amount_per_share.round_dp(6).to_string(),
                shares_estimate: shares.round_dp(6).to_string(),
                cash_amount_estimate: cash.round_dp(6).to_string(),
                ttm_yield_pct,
                currency: r.currency.clone(),
                notes: r.notes.clone(),
            }
        })
        .collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!(
        "{:<5} {:<10} {:<12} {:<12} {:>12} {:>12} {:>14} {:>10}",
        "ID", "Symbol", "Ex-Date", "Pay-Date", "Amt/Share", "Shares", "Cash Est.", "TTM Yld%"
    );
    println!("{}", "─".repeat(96));
    for r in &output {
        println!(
            "{:<5} {:<10} {:<12} {:<12} {:>12} {:>12} {:>14} {:>10}",
            r.id,
            r.symbol,
            r.ex_date.as_deref().unwrap_or("—"),
            r.pay_date,
            r.amount_per_share,
            r.shares_estimate,
            r.cash_amount_estimate,
            r.ttm_yield_pct.as_deref().unwrap_or("—"),
        );
    }
    Ok(())
}

fn run_remove(backend: &BackendConnection, args: &DividendsArgs) -> Result<()> {
    let id = args
        .value
        .as_deref()
        .ok_or_else(|| anyhow!("Usage: pftui dividends remove ID"))?
        .parse::<i64>()
        .map_err(|_| anyhow!("ID must be an integer"))?;

    if !dividends_db::remove_backend(backend, id)? {
        bail!("No dividend found with id {}", id);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "removed": id }))?
        );
    } else {
        println!("Removed dividend #{}", id);
    }
    Ok(())
}

fn current_shares_by_symbol(
    backend: &BackendConnection,
) -> Result<std::collections::HashMap<String, Decimal>> {
    let txs = transactions::list_transactions_backend(backend)?;
    let mut out: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();
    for tx in txs {
        let entry = out.entry(tx.symbol.to_uppercase()).or_insert(Decimal::ZERO);
        match tx.tx_type {
            TxType::Buy => *entry += tx.quantity,
            TxType::Sell => *entry -= tx.quantity,
        }
    }
    Ok(out)
}

fn trailing_12m_div_per_share(
    backend: &BackendConnection,
) -> Result<std::collections::HashMap<String, Decimal>> {
    let cutoff = (Utc::now().date_naive() - Duration::days(365))
        .format("%Y-%m-%d")
        .to_string();
    let mut out: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();
    for row in dividends_db::list_backend(backend, None)? {
        if row.pay_date >= cutoff {
            *out.entry(row.symbol).or_insert(Decimal::ZERO) += row.amount_per_share;
        }
    }
    Ok(out)
}

fn parse_decimal(s: &str) -> Result<Decimal> {
    s.trim()
        .trim_end_matches('%')
        .parse::<Decimal>()
        .map_err(|_| anyhow!("invalid decimal value '{}'", s))
}

fn validate_date(s: &str) -> Result<()> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| anyhow!("invalid date '{}', expected YYYY-MM-DD", s))
}
