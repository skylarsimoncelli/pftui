use anyhow::Result;
use serde_json::json;

use crate::data::cot::{interpret_managed_money, symbol_to_cftc_code, COT_CONTRACTS};
use crate::db::backend::BackendConnection;
use crate::db::cot_cache;

#[derive(Debug, Clone, serde::Serialize)]
struct CotAnalysisRow {
    symbol: String,
    name: String,
    category: String,
    report_date: String,
    net_long: i64,
    percentile_1y: f64,
    percentile_3y: f64,
    z_score: f64,
    extreme: bool,
}

pub fn run(backend: &BackendConnection, symbol: Option<&str>, json: bool) -> Result<()> {
    if let Some(symbol) = symbol {
        let row = analyze_symbol(backend, symbol)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&row)?);
        } else {
            print_single(&row);
        }
        return Ok(());
    }

    let mut rows = Vec::new();
    for contract in COT_CONTRACTS {
        if let Ok(row) = analyze_symbol(backend, contract.symbol) {
            rows.push(row);
        }
    }

    if json {
        let payload = rows
            .iter()
            .map(|row| {
                (
                    normalize_key(&row.name),
                    json!({
                        "symbol": row.symbol,
                        "report_date": row.report_date,
                        "net_long": row.net_long,
                        "percentile_1y": row.percentile_1y,
                        "percentile_3y": row.percentile_3y,
                        "z_score": row.z_score,
                        "extreme": row.extreme,
                    }),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if rows.is_empty() {
        println!("No cached COT history found. Run `pftui data refresh` first.");
    } else {
        print_overview(&rows);
    }

    Ok(())
}

fn analyze_symbol(backend: &BackendConnection, symbol: &str) -> Result<CotAnalysisRow> {
    let cftc_code = symbol_to_cftc_code(symbol).ok_or_else(|| {
        anyhow::anyhow!(
            "Symbol '{}' is not tracked for COT data. Supported: GC=F, SI=F, CL=F, BTC",
            symbol
        )
    })?;
    let contract = COT_CONTRACTS
        .iter()
        .find(|contract| contract.cftc_code == cftc_code)
        .ok_or_else(|| anyhow::anyhow!("missing COT contract metadata for {}", symbol))?;
    let history = cot_cache::get_history_backend(backend, cftc_code, 156)?;
    let latest = history.first().ok_or_else(|| {
        anyhow::anyhow!(
            "No cached COT history found for {}. Run `pftui data refresh` first.",
            symbol
        )
    })?;
    let managed_money_history: Vec<i64> = history.iter().map(|row| row.managed_money_net).collect();
    let interpretation = interpret_managed_money(&managed_money_history).ok_or_else(|| {
        anyhow::anyhow!(
            "Not enough cached COT history for {}. Run `pftui data refresh` again after more reports are available.",
            symbol
        )
    })?;

    Ok(CotAnalysisRow {
        symbol: contract.symbol.to_string(),
        name: contract.name.to_string(),
        category: contract.category.to_string(),
        report_date: latest.report_date.clone(),
        net_long: latest.managed_money_net,
        percentile_1y: round1(interpretation.percentile_1y),
        percentile_3y: round1(interpretation.percentile_3y),
        z_score: round1(interpretation.z_score),
        extreme: interpretation.extreme,
    })
}

fn print_single(row: &CotAnalysisRow) {
    println!("\nCOT Positioning\n");
    println!("  Asset: {}", row.name);
    println!("  Symbol: {}", row.symbol);
    println!("  Report date: {}", row.report_date);
    println!("  Managed money net: {}", format_signed(row.net_long));
    println!(
        "  Percentiles: 1Y {:.1} | 3Y {:.1}",
        row.percentile_1y, row.percentile_3y
    );
    println!("  Z-score: {:.1}", row.z_score);
    println!("  Extreme: {}", if row.extreme { "yes" } else { "no" });
}

fn print_overview(rows: &[CotAnalysisRow]) {
    println!("COT Positioning");
    println!();
    println!(
        "  {:<8} {:<12} {:>12} {:>8} {:>8} {:>8} {:>8}",
        "Symbol", "Asset", "Net", "1Y %", "3Y %", "Z", "Extreme"
    );
    for row in rows {
        println!(
            "  {:<8} {:<12} {:>12} {:>8.1} {:>8.1} {:>8.1} {:>8}",
            row.symbol,
            shorten_name(&row.name),
            format_signed(row.net_long),
            row.percentile_1y,
            row.percentile_3y,
            row.z_score,
            if row.extreme { "yes" } else { "no" }
        );
    }
}

fn normalize_key(name: &str) -> String {
    name.split_whitespace()
        .next()
        .unwrap_or(name)
        .to_lowercase()
        .replace('/', "_")
}

fn shorten_name(name: &str) -> &str {
    if name.starts_with("WTI") {
        "WTI Crude"
    } else if name.starts_with("Gold") {
        "Gold"
    } else if name.starts_with("Silver") {
        "Silver"
    } else if name.starts_with("Bitcoin") {
        "Bitcoin"
    } else {
        name
    }
}

fn format_signed(value: i64) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        value.to_string()
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_key_uses_leading_asset_name() {
        assert_eq!(normalize_key("Gold Futures"), "gold");
        assert_eq!(normalize_key("WTI Crude Oil Futures"), "wti");
    }

    #[test]
    fn format_signed_adds_plus_for_positive() {
        assert_eq!(format_signed(12_000), "+12000");
        assert_eq!(format_signed(-8_000), "-8000");
    }
}
