use std::collections::HashMap;

use anyhow::Result;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;

use crate::analytics::drawdown::{self, DrawdownReport, DrawdownSummary};
use crate::config::{Config, PortfolioMode};
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_prices_at_date_backend;
use crate::db::snapshots::get_all_portfolio_snapshots_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, Position};

pub fn run(backend: &BackendConnection, config: &Config, json: bool) -> Result<()> {
    let Some(report) = build_report_backend(backend, config)? else {
        if json {
            println!(
                r#"{{"error":"no_drawdown_data","message":"No portfolio snapshots found. Run pftui data refresh to record daily snapshots."}}"#
            );
        } else {
            println!("No portfolio snapshots found.");
            println!("Run `pftui data refresh` to start recording daily snapshots.");
        }
        return Ok(());
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_report(config, &report);
    Ok(())
}

pub fn build_report_backend(
    backend: &BackendConnection,
    config: &Config,
) -> Result<Option<DrawdownReport>> {
    let snapshots = get_all_portfolio_snapshots_backend(backend)?;
    let today = Utc::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => load_full_positions(backend)?,
        PortfolioMode::Percentage => Vec::new(),
    };
    let current_value = if positions.is_empty() {
        None
    } else {
        Some(positions.iter().filter_map(|p| p.current_value).sum())
    };

    let decomposition = if positions.is_empty() {
        None
    } else {
        let yesterday = today - Duration::days(1);
        let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
        let symbols: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();
        let previous_prices =
            get_prices_at_date_backend(backend, &symbols, &yesterday_str).unwrap_or_default();
        drawdown::compute_latest_decomposition(
            &positions,
            &previous_prices,
            &today_str,
            &yesterday_str,
        )
    };

    Ok(drawdown::compute_drawdown_report(
        &snapshots,
        Some(&today_str),
        current_value,
        decomposition,
    ))
}

pub fn format_summary_line(summary: &DrawdownSummary, currency_symbol: &str) -> String {
    format!(
        "Drawdown: {} from {} high {}{}  |  MTD max: {}  |  YTD max: {}",
        format_pct(summary.current_dd_from_local_high_pct),
        summary.local_high_date,
        currency_symbol,
        format_with_commas(summary.local_high_value, 2),
        format_pct(summary.max_dd_mtd_pct),
        format_pct(summary.max_dd_ytd_pct),
    )
}

fn load_full_positions(backend: &BackendConnection) -> Result<Vec<Position>> {
    let txs = list_transactions_backend(backend)?;
    if txs.is_empty() {
        return Ok(Vec::new());
    }

    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect();
    for tx in &txs {
        if tx.category == AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    Ok(compute_positions(&txs, &prices, &fx_rates))
}

fn print_report(config: &Config, report: &DrawdownReport) {
    let csym = config.currency_symbol();
    println!("Portfolio Drawdown — last 90 days");
    println!("{}", format_summary_line(&report.summary, csym));
    println!();

    println!(
        "{:<12} {:>14} {:>14} {:>12} {:>14}",
        "Date", "Value", "High Date", "DD %", "DD Value"
    );
    println!("{}", "-".repeat(72));
    for point in &report.series {
        println!(
            "{:<12} {:>14} {:>14} {:>12} {:>14}",
            point.date,
            format!("{}{}", csym, format_with_commas(point.total_value, 2)),
            point.local_high_date,
            format_pct(point.drawdown_pct),
            format_currency(csym, point.drawdown_value, 2),
        );
    }

    if let Some(decomp) = &report.latest_decomposition {
        println!();
        println!(
            "Latest move decomposition ({} vs {}): {}{} ({})",
            decomp.as_of,
            decomp.previous_date,
            if decomp.total_daily_change >= 0.0 {
                "+"
            } else {
                ""
            },
            format_currency(csym, decomp.total_daily_change, 2),
            format_pct(decomp.total_daily_change_pct)
        );

        if decomp.positions.is_empty() {
            println!("No non-zero position moves available.");
        } else {
            println!(
                "{:<10} {:>10} {:>10} {:>12} {:>14}",
                "Symbol", "Move %", "Weight", "Contrib", "Value Chg"
            );
            println!("{}", "-".repeat(62));
            for row in &decomp.positions {
                println!(
                    "{:<10} {:>10} {:>10} {:>12} {:>14}",
                    row.symbol,
                    format_pct(row.daily_change_pct),
                    format_pct(row.weight_pct),
                    format_pct(row.contribution_pct),
                    format_signed_currency(csym, row.change_value, 2),
                );
            }
        }
    }
}

fn format_pct(value: f64) -> String {
    if value.abs() < 0.005 {
        "0.00%".to_string()
    } else {
        format!("{value:+.2}%")
    }
}

fn format_with_commas(value: f64, dp: usize) -> String {
    let rounded = format!("{:.prec$}", value, prec = dp);
    let (integer_part, decimal_part) = rounded
        .split_once('.')
        .map(|(i, d)| (i, Some(d)))
        .unwrap_or((rounded.as_str(), None));
    let (sign, digits) = integer_part
        .strip_prefix('-')
        .map(|rest| ("-", rest))
        .unwrap_or(("", integer_part));

    let mut grouped = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let mut out = format!("{}{}", sign, grouped.chars().rev().collect::<String>());
    if let Some(decimal) = decimal_part {
        out.push('.');
        out.push_str(decimal);
    }
    out
}

fn format_currency(symbol: &str, value: f64, dp: usize) -> String {
    if value < 0.0 {
        format!("-{}{}", symbol, format_with_commas(value.abs(), dp))
    } else {
        format!("{}{}", symbol, format_with_commas(value, dp))
    }
}

fn format_signed_currency(symbol: &str, value: f64, dp: usize) -> String {
    if value >= 0.0 {
        format!("+{}", format_currency(symbol, value, dp))
    } else {
        format_currency(symbol, value, dp)
    }
}
