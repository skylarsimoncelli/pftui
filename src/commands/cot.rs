use anyhow::Result;
use serde_json::json;

use crate::data::cot::{
    fetch_historical_reports, fetch_latest_report, symbol_to_cftc_code, CotContract,
    CotReport, COT_CONTRACTS,
};

/// Run the `pftui cot` command.
pub fn run(
    symbol: Option<&str>,
    weeks: usize,
    json: bool,
) -> Result<()> {
    if let Some(sym) = symbol {
        // Fetch for specific symbol
        let cftc_code = symbol_to_cftc_code(sym)
            .ok_or_else(|| anyhow::anyhow!(
                "Symbol '{}' is not tracked for COT data. Supported: GC=F, SI=F, CL=F, BTC",
                sym
            ))?;

        let contract = COT_CONTRACTS
            .iter()
            .find(|c| c.cftc_code == cftc_code)
            .unwrap();

        if weeks == 1 {
            let report = fetch_latest_report(cftc_code)?;
            if json {
                print_json_single(&report, contract)?;
            } else {
                print_single(&report, contract);
            }
        } else {
            let reports = fetch_historical_reports(cftc_code, weeks)?;
            if json {
                print_json_historical(&reports, contract)?;
            } else {
                print_historical(&reports, contract);
            }
        }
    } else {
        // Fetch for all tracked symbols
        fetch_all(json)?;
    }

    Ok(())
}

/// Fetch and display latest COT data for all tracked contracts.
fn fetch_all(json: bool) -> Result<()> {
    let mut results = Vec::new();

    for contract in COT_CONTRACTS {
        match fetch_latest_report(contract.cftc_code) {
            Ok(report) => results.push((contract, Some(report))),
            Err(_) => results.push((contract, None)),
        }
    }

    if json {
        print_json_all(&results)?;
    } else {
        print_table_all(&results);
    }

    Ok(())
}

/// Print a single COT report (non-JSON).
fn print_single(report: &CotReport, contract: &CotContract) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  CFTC Commitments of Traders — {}  ║", contract.name);
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("Symbol:        {}", contract.symbol);
    println!("Report Date:   {}", report.report_date);
    println!("Open Interest: {}\n", format_with_commas(report.open_interest));

    println!("Managed Money (Speculators):");
    println!("  Long:        {:>12}", format_with_commas(report.managed_money_long));
    println!("  Short:       {:>12}", format_with_commas(report.managed_money_short));
    println!("  Net Long:    {:>12}\n", format_with_commas(report.managed_money_net));

    println!("Commercials (Hedgers):");
    println!("  Long:        {:>12}", format_with_commas(report.commercial_long));
    println!("  Short:       {:>12}", format_with_commas(report.commercial_short));
    println!("  Net Long:    {:>12}", format_with_commas(report.commercial_net));
}

/// Print historical COT reports (non-JSON).
fn print_historical(reports: &[CotReport], contract: &CotContract) {
    if reports.is_empty() {
        println!("No COT data found for {}", contract.symbol);
        return;
    }

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  CFTC COT — {} ({} weeks)  ║", contract.name, reports.len());
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Header
    println!(
        "{:<12}  {:>10}  {:>12}  {:>12}  {:>12}",
        "Date", "OpenInt", "MM Net", "MM Δ", "Comm Net"
    );
    println!("{}", "─".repeat(68));

    // Rows (reverse chronological)
    for (i, report) in reports.iter().enumerate() {
        let mm_change = if i < reports.len() - 1 {
            let prev = &reports[i + 1];
            report.managed_money_change(prev)
        } else {
            0
        };

        let mm_change_str = if mm_change > 0 {
            format!("+{}", format_with_commas(mm_change))
        } else {
            format_with_commas(mm_change)
        };

        println!(
            "{:<12}  {:>10}  {:>12}  {:>12}  {:>12}",
            report.report_date,
            format_with_commas_short(report.open_interest),
            format_with_commas(report.managed_money_net),
            mm_change_str,
            format_with_commas(report.commercial_net),
        );
    }
}

/// Print all tracked contracts as a summary table.
fn print_table_all(results: &[(&CotContract, Option<CotReport>)]) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           CFTC Commitments of Traders — All Tracked         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Header
    println!(
        "{:<8}  {:<25}  {:<12}  {:>12}  {:>12}",
        "Symbol", "Name", "Date", "MM Net", "Comm Net"
    );
    println!("{}", "─".repeat(78));

    // Rows
    for (contract, report_opt) in results {
        match report_opt {
            Some(report) => {
                println!(
                    "{:<8}  {:<25}  {:<12}  {:>12}  {:>12}",
                    contract.symbol,
                    contract.name,
                    report.report_date,
                    format_with_commas(report.managed_money_net),
                    format_with_commas(report.commercial_net),
                );
            }
            None => {
                println!(
                    "{:<8}  {:<25}  {:<12}  {:>12}  {:>12}",
                    contract.symbol, contract.name, "---", "N/A", "N/A"
                );
            }
        }
    }
}

/// Print a single COT report as JSON.
fn print_json_single(report: &CotReport, contract: &CotContract) -> Result<()> {
    let output = json!({
        "symbol": contract.symbol,
        "name": contract.name,
        "category": contract.category,
        "report_date": report.report_date,
        "open_interest": report.open_interest,
        "managed_money": {
            "long": report.managed_money_long,
            "short": report.managed_money_short,
            "net": report.managed_money_net,
        },
        "commercial": {
            "long": report.commercial_long,
            "short": report.commercial_short,
            "net": report.commercial_net,
        },
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print historical COT reports as JSON.
fn print_json_historical(reports: &[CotReport], contract: &CotContract) -> Result<()> {
    let output = json!({
        "symbol": contract.symbol,
        "name": contract.name,
        "category": contract.category,
        "weeks": reports.len(),
        "reports": reports.iter().enumerate().map(|(i, report)| {
            let mm_change = if i < reports.len() - 1 {
                let prev = &reports[i + 1];
                report.managed_money_change(prev)
            } else {
                0
            };

            json!({
                "report_date": report.report_date,
                "open_interest": report.open_interest,
                "managed_money": {
                    "long": report.managed_money_long,
                    "short": report.managed_money_short,
                    "net": report.managed_money_net,
                    "change": mm_change,
                },
                "commercial": {
                    "long": report.commercial_long,
                    "short": report.commercial_short,
                    "net": report.commercial_net,
                },
            })
        }).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print all tracked contracts as JSON.
fn print_json_all(results: &[(&CotContract, Option<CotReport>)]) -> Result<()> {
    let output = json!(results
        .iter()
        .filter_map(|(contract, report_opt)| {
            report_opt.as_ref().map(|report| {
                json!({
                    "symbol": contract.symbol,
                    "name": contract.name,
                    "category": contract.category,
                    "report_date": report.report_date,
                    "open_interest": report.open_interest,
                    "managed_money": {
                        "long": report.managed_money_long,
                        "short": report.managed_money_short,
                        "net": report.managed_money_net,
                    },
                    "commercial": {
                        "long": report.commercial_long,
                        "short": report.commercial_short,
                        "net": report.commercial_net,
                    },
                })
            })
        })
        .collect::<Vec<_>>());

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Format an integer with commas.
fn format_with_commas(n: i64) -> String {
    let s = n.abs().to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().rev().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }

    let formatted: String = result.chars().rev().collect();
    if n < 0 {
        format!("-{}", formatted)
    } else {
        formatted
    }
}

/// Format an integer with K/M suffix for compact display.
fn format_with_commas_short(n: i64) -> String {
    if n.abs() >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n.abs() >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_with_commas() {
        assert_eq!(format_with_commas(1000), "1,000");
        assert_eq!(format_with_commas(1000000), "1,000,000");
        assert_eq!(format_with_commas(-50000), "-50,000");
        assert_eq!(format_with_commas(123), "123");
    }

    #[test]
    fn test_format_with_commas_short() {
        assert_eq!(format_with_commas_short(500), "500");
        assert_eq!(format_with_commas_short(1500), "1.5K");
        assert_eq!(format_with_commas_short(50000), "50.0K");
        assert_eq!(format_with_commas_short(1500000), "1.5M");
    }
}
