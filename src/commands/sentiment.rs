//! Unified sentiment command: Fear & Greed indices + COT positioning.
//!
//! Combines:
//! - Crypto Fear & Greed Index (Alternative.me)
//! - Traditional Fear & Greed Index (derived from VIX + market indicators)
//! - CFTC Commitments of Traders (COT) positioning for commodities
//!
//! Usage:
//! - `pftui sentiment` — overview of all indices + COT positioning for tracked assets
//! - `pftui sentiment --history 30` — 30-day trend for F&G indices
//! - `pftui sentiment GC=F` — detailed COT positioning for gold
//! - `pftui sentiment --json` — JSON output for agent consumption

use anyhow::Result;
use serde_json::json;

use crate::data::cot::{
    fetch_historical_reports, fetch_latest_report, symbol_to_cftc_code, CotContract,
    CotReport, COT_CONTRACTS,
};
use crate::data::sentiment::{fetch_crypto_fng, fetch_traditional_fng, SentimentIndex};

/// Run the `pftui sentiment` command.
pub fn run(symbol: Option<&str>, history: Option<usize>, json: bool) -> Result<()> {
    if let Some(sym) = symbol {
        // Detailed view for a specific symbol (COT positioning only)
        run_symbol_detail(sym, history.unwrap_or(1), json)
    } else if let Some(days) = history {
        // Historical trend view for F&G indices
        run_history(days, json)
    } else {
        // Overview: F&G + COT positioning for all tracked assets
        run_overview(json)
    }
}

/// Overview mode: show F&G indices + COT positioning summary.
fn run_overview(json: bool) -> Result<()> {
    // Fetch Fear & Greed indices
    let crypto_fng = fetch_crypto_fng().ok();
    let trad_fng = fetch_traditional_fng().ok();

    // Fetch COT positioning for all tracked contracts
    let mut cot_results = Vec::new();
    for contract in COT_CONTRACTS {
        match fetch_latest_report(contract.cftc_code) {
            Ok(report) => cot_results.push((contract, Some(report))),
            Err(_) => cot_results.push((contract, None)),
        }
    }

    if json {
        print_overview_json(&crypto_fng, &trad_fng, &cot_results)?;
    } else {
        print_overview(&crypto_fng, &trad_fng, &cot_results);
    }

    Ok(())
}

/// Print overview in human-readable format.
fn print_overview(
    crypto_fng: &Option<SentimentIndex>,
    trad_fng: &Option<SentimentIndex>,
    cot_results: &[(&CotContract, Option<CotReport>)],
) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              Market Sentiment & Positioning                 ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Fear & Greed Indices
    println!("┌─ FEAR & GREED INDICES ─────────────────────────────────────┐");
    
    if let Some(idx) = crypto_fng {
        let emoji = sentiment_emoji(idx.value);
        println!(
            "│ Crypto:       {} {:>3}/100  {}",
            emoji,
            idx.value,
            idx.classification
        );
    } else {
        println!("│ Crypto:       ⚠️  unavailable");
    }

    if let Some(idx) = trad_fng {
        let emoji = sentiment_emoji(idx.value);
        println!(
            "│ Traditional:  {} {:>3}/100  {}",
            emoji,
            idx.value,
            idx.classification
        );
    } else {
        println!("│ Traditional:  ⚠️  unavailable");
    }
    
    println!("└────────────────────────────────────────────────────────────┘\n");

    // COT Positioning
    println!("┌─ COMMITMENTS OF TRADERS (Latest Week) ────────────────────┐");
    println!("│ Asset            Managed Money       Commercial          │");
    println!("│                  Net     Signal       Net     Signal      │");
    println!("├────────────────────────────────────────────────────────────┤");

    for (contract, report_opt) in cot_results {
        if let Some(report) = report_opt {
            let mm_signal = cot_signal(report.managed_money_net, report.open_interest);
            let comm_signal = cot_signal(report.commercial_net, report.open_interest);
            
            println!(
                "│ {:12}  {:>8}  {}       {:>8}  {}      │",
                shorten_name(contract.name),
                format_cot_net(report.managed_money_net),
                mm_signal,
                format_cot_net(report.commercial_net),
                comm_signal,
            );
        } else {
            println!(
                "│ {:12}  ⚠️  unavailable                            │",
                shorten_name(contract.name)
            );
        }
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
    println!("📊 Use `pftui sentiment <symbol>` for detailed positioning");
    println!("📈 Use `pftui sentiment --history N` for F&G trend");
}

/// Print overview in JSON format.
fn print_overview_json(
    crypto_fng: &Option<SentimentIndex>,
    trad_fng: &Option<SentimentIndex>,
    cot_results: &[(&CotContract, Option<CotReport>)],
) -> Result<()> {
    let crypto_json = crypto_fng.as_ref().map(|idx| {
        json!({
            "type": idx.index_type,
            "value": idx.value,
            "classification": idx.classification,
            "timestamp": idx.timestamp,
        })
    });

    let trad_json = trad_fng.as_ref().map(|idx| {
        json!({
            "type": idx.index_type,
            "value": idx.value,
            "classification": idx.classification,
            "timestamp": idx.timestamp,
        })
    });

    let cot_json: Vec<_> = cot_results
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
                    }
                })
            })
        })
        .collect();

    let output = json!({
        "fear_and_greed": {
            "crypto": crypto_json,
            "traditional": trad_json,
        },
        "cot_positioning": cot_json,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Symbol detail mode: show detailed COT positioning for one asset.
fn run_symbol_detail(symbol: &str, weeks: usize, json: bool) -> Result<()> {
    let cftc_code = symbol_to_cftc_code(symbol).ok_or_else(|| {
        anyhow::anyhow!(
            "Symbol '{}' is not tracked for COT data. Supported: GC=F, SI=F, CL=F, BTC",
            symbol
        )
    })?;

    let contract = COT_CONTRACTS
        .iter()
        .find(|c| c.cftc_code == cftc_code)
        .unwrap();

    if weeks == 1 {
        let report = fetch_latest_report(cftc_code)?;
        if json {
            print_symbol_detail_json(&report, contract)?;
        } else {
            print_symbol_detail(&report, contract);
        }
    } else {
        let reports = fetch_historical_reports(cftc_code, weeks)?;
        if json {
            print_symbol_history_json(&reports, contract)?;
        } else {
            print_symbol_history(&reports, contract);
        }
    }

    Ok(())
}

/// Print detailed COT positioning for a single symbol (latest week).
fn print_symbol_detail(report: &CotReport, contract: &CotContract) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  CFTC Commitments of Traders — {}  ║", contract.name);
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("Symbol:        {}", contract.symbol);
    println!("Report Date:   {}", report.report_date);
    println!("Open Interest: {}\n", format_with_commas(report.open_interest));

    println!("┌─ MANAGED MONEY (Speculators) ──────────────────────────────┐");
    println!("│ Long:   {:>12}                                      │", format_with_commas(report.managed_money_long));
    println!("│ Short:  {:>12}                                      │", format_with_commas(report.managed_money_short));
    println!("│ Net:    {:>12}  {}                              │", 
        format_cot_net(report.managed_money_net),
        cot_signal(report.managed_money_net, report.open_interest)
    );
    println!("└────────────────────────────────────────────────────────────┘\n");

    println!("┌─ COMMERCIALS (Producers/Hedgers) ──────────────────────────┐");
    println!("│ Long:   {:>12}                                      │", format_with_commas(report.commercial_long));
    println!("│ Short:  {:>12}                                      │", format_with_commas(report.commercial_short));
    println!("│ Net:    {:>12}  {}                              │", 
        format_cot_net(report.commercial_net),
        cot_signal(report.commercial_net, report.open_interest)
    );
    println!("└────────────────────────────────────────────────────────────┘\n");

    println!("💡 Managed Money = trend followers, momentum traders");
    println!("💡 Commercials = smart money, typically contrarian signal");
}

/// Print symbol detail in JSON format.
fn print_symbol_detail_json(report: &CotReport, contract: &CotContract) -> Result<()> {
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
        }
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print historical COT positioning for a single symbol.
fn print_symbol_history(reports: &[CotReport], contract: &CotContract) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  CFTC COT History — {}  ║", contract.name);
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("│ Date       │  Open Int  │ MM Net     │ Comm Net   │ MM Δ     │ Comm Δ   │");
    println!("├────────────┼────────────┼────────────┼────────────┼──────────┼──────────┤");

    for (i, report) in reports.iter().enumerate() {
        let mm_change = if i > 0 {
            format!("{:+}", report.managed_money_change(&reports[i - 1]))
        } else {
            "—".to_string()
        };

        let comm_change = if i > 0 {
            format!("{:+}", report.commercial_change(&reports[i - 1]))
        } else {
            "—".to_string()
        };

        println!(
            "│ {} │ {:>10} │ {:>10} │ {:>10} │ {:>8} │ {:>8} │",
            report.report_date,
            format_with_commas(report.open_interest),
            format_cot_net(report.managed_money_net),
            format_cot_net(report.commercial_net),
            mm_change,
            comm_change,
        );
    }

    println!("└────────────┴────────────┴────────────┴────────────┴──────────┴──────────┘");
}

/// Print symbol history in JSON format.
fn print_symbol_history_json(reports: &[CotReport], contract: &CotContract) -> Result<()> {
    let history: Vec<_> = reports
        .iter()
        .enumerate()
        .map(|(i, report)| {
            let mm_change = if i > 0 {
                Some(report.managed_money_change(&reports[i - 1]))
            } else {
                None
            };

            let comm_change = if i > 0 {
                Some(report.commercial_change(&reports[i - 1]))
            } else {
                None
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
                    "change": comm_change,
                }
            })
        })
        .collect();

    let output = json!({
        "symbol": contract.symbol,
        "name": contract.name,
        "category": contract.category,
        "history": history,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Historical trend mode: show F&G index trend over N days.
fn run_history(days: usize, json: bool) -> Result<()> {
    // For now, just show the current F&G values
    // Future: fetch historical F&G data from Alternative.me API (?limit=N)
    let crypto_fng = fetch_crypto_fng().ok();
    let trad_fng = fetch_traditional_fng().ok();

    if json {
        let output = json!({
            "note": "Historical F&G data not yet implemented",
            "current": {
                "crypto": crypto_fng.as_ref().map(|idx| json!({
                    "value": idx.value,
                    "classification": idx.classification,
                    "timestamp": idx.timestamp,
                })),
                "traditional": trad_fng.as_ref().map(|idx| json!({
                    "value": idx.value,
                    "classification": idx.classification,
                    "timestamp": idx.timestamp,
                })),
            }
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("📈 Historical F&G trend (--history) not yet implemented");
        println!("   Showing current values:\n");

        if let Some(idx) = crypto_fng {
            println!(
                "Crypto F&G:       {} {}/100  {}",
                sentiment_emoji(idx.value),
                idx.value,
                idx.classification
            );
        }

        if let Some(idx) = trad_fng {
            println!(
                "Traditional F&G:  {} {}/100  {}",
                sentiment_emoji(idx.value),
                idx.value,
                idx.classification
            );
        }

        println!("\n💡 Historical trend will show {}-day F&G sparklines + trend arrows", days);
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Sentiment emoji based on F&G index value.
fn sentiment_emoji(value: u8) -> &'static str {
    match value {
        0..=24 => "🔴", // Extreme Fear
        25..=44 => "🟠", // Fear
        45..=55 => "🟡", // Neutral
        56..=74 => "🟢", // Greed
        _ => "🟢",       // Extreme Greed
    }
}

/// COT positioning signal emoji.
fn cot_signal(net: i64, open_interest: i64) -> &'static str {
    let pct = (net as f64 / open_interest as f64) * 100.0;
    
    if pct.abs() < 10.0 {
        "🟡" // Neutral
    } else if pct > 25.0 {
        "🔴" // Extreme long (contrarian bearish)
    } else if pct < -25.0 {
        "🟢" // Extreme short (contrarian bullish)
    } else if pct > 0.0 {
        "🟠" // Moderate long
    } else {
        "🟢" // Moderate short
    }
}

/// Format COT net positioning with + sign for longs.
fn format_cot_net(net: i64) -> String {
    if net >= 0 {
        format!("+{}", format_with_commas(net))
    } else {
        format_with_commas(net)
    }
}

/// Format number with commas.
fn format_with_commas(n: i64) -> String {
    let s = n.abs().to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    if n < 0 {
        result.insert(0, '-');
    }
    result
}

/// Shorten contract name for table display.
fn shorten_name(name: &str) -> String {
    name.replace(" Futures", "")
        .chars()
        .take(12)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentiment_emoji() {
        assert_eq!(sentiment_emoji(10), "🔴"); // Extreme Fear
        assert_eq!(sentiment_emoji(30), "🟠"); // Fear
        assert_eq!(sentiment_emoji(50), "🟡"); // Neutral
        assert_eq!(sentiment_emoji(65), "🟢"); // Greed
        assert_eq!(sentiment_emoji(90), "🟢"); // Extreme Greed
    }

    #[test]
    fn test_cot_signal() {
        assert_eq!(cot_signal(5000, 100000), "🟡"); // 5% - neutral
        assert_eq!(cot_signal(30000, 100000), "🔴"); // 30% - extreme long
        assert_eq!(cot_signal(-30000, 100000), "🟢"); // -30% - extreme short
        assert_eq!(cot_signal(15000, 100000), "🟠"); // 15% - moderate long
        assert_eq!(cot_signal(-15000, 100000), "🟢"); // -15% - moderate short
    }

    #[test]
    fn test_format_with_commas() {
        assert_eq!(format_with_commas(1000), "1,000");
        assert_eq!(format_with_commas(-1000000), "-1,000,000");
        assert_eq!(format_with_commas(42), "42");
    }

    #[test]
    fn test_format_cot_net() {
        assert_eq!(format_cot_net(5000), "+5,000");
        assert_eq!(format_cot_net(-5000), "-5,000");
        assert_eq!(format_cot_net(0), "+0");
    }
}
