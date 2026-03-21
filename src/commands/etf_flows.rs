use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::data::onchain::{fetch_etf_flows, EtfFlow};

#[derive(Serialize)]
struct EtfFlowsOutput {
    date_range: String,
    total_flows: Vec<DailyTotal>,
    fund_flows: Vec<FundFlow>,
}

#[derive(Serialize)]
struct DailyTotal {
    date: String,
    total_btc: f64,
    total_usd: f64,
}

#[derive(Serialize)]
struct FundFlow {
    fund: String,
    date: String,
    flow_btc: f64,
    flow_usd: f64,
}

pub fn run(days: u16, fund_filter: Option<String>, json: bool) -> Result<()> {
    let flows = fetch_etf_flows()?;

    if flows.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&EtfFlowsOutput {
                    date_range: "no data".to_string(),
                    total_flows: vec![],
                    fund_flows: vec![],
                })?
            );
        } else {
            println!("No ETF flow data available.");
        }
        return Ok(());
    }

    // Determine date range
    let today = Utc::now().date_naive();
    let cutoff = today - chrono::Duration::days(i64::from(days) - 1);
    let today_str = today.format("%Y-%m-%d").to_string();
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

    // Filter by date range (dates are strings in YYYY-MM-DD format, lexicographically sortable)
    let filtered: Vec<_> = flows
        .iter()
        .filter(|f| f.date.as_str() >= cutoff_str.as_str() && f.date.as_str() <= today_str.as_str())
        .collect();

    if filtered.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&EtfFlowsOutput {
                    date_range: format!("{} to {}", cutoff, today),
                    total_flows: vec![],
                    fund_flows: vec![],
                })?
            );
        } else {
            println!("No ETF flow data in the last {} days.", days);
        }
        return Ok(());
    }

    // Filter by fund if specified
    let fund_filtered: Vec<_> = if let Some(ref fund_name) = fund_filter {
        filtered
            .into_iter()
            .filter(|f| f.fund.eq_ignore_ascii_case(fund_name))
            .collect()
    } else {
        filtered
    };

    if fund_filtered.is_empty() {
        if let Some(ref fund_name) = fund_filter {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&EtfFlowsOutput {
                        date_range: format!("{} to {}", cutoff, today),
                        total_flows: vec![],
                        fund_flows: vec![],
                    })?
                );
            } else {
                println!(
                    "No data for fund '{}' in the last {} days.",
                    fund_name, days
                );
            }
            return Ok(());
        }
    }

    if json {
        // Build JSON output
        let mut daily_totals: std::collections::HashMap<String, (f64, f64)> =
            std::collections::HashMap::new();

        for flow in &fund_filtered {
            let entry = daily_totals.entry(flow.date.clone()).or_insert((0.0, 0.0));
            entry.0 += flow.net_flow_btc;
            entry.1 += flow.net_flow_usd;
        }

        let mut total_flows: Vec<DailyTotal> = daily_totals
            .into_iter()
            .map(|(date, (total_btc, total_usd))| DailyTotal {
                date,
                total_btc,
                total_usd,
            })
            .collect();
        total_flows.sort_by(|a, b| b.date.cmp(&a.date));

        let fund_flows: Vec<FundFlow> = fund_filtered
            .iter()
            .map(|f| FundFlow {
                fund: f.fund.clone(),
                date: f.date.clone(),
                flow_btc: f.net_flow_btc,
                flow_usd: f.net_flow_usd,
            })
            .collect();

        let output = EtfFlowsOutput {
            date_range: format!("{} to {}", cutoff, today),
            total_flows,
            fund_flows,
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Human-readable output
        println!("BTC ETF Flows — {} to {}", cutoff_str, today_str);
        println!();

        // Group by date for daily totals
        let mut daily_totals: std::collections::HashMap<String, (f64, f64)> =
            std::collections::HashMap::new();

        for flow in &fund_filtered {
            let entry = daily_totals.entry(flow.date.clone()).or_insert((0.0, 0.0));
            entry.0 += flow.net_flow_btc;
            entry.1 += flow.net_flow_usd;
        }

        let mut dates: Vec<String> = daily_totals.keys().cloned().collect();
        dates.sort();
        dates.reverse();

        if fund_filter.is_none() {
            println!("Daily Totals:");
            println!("{:<12} {:>15} {:>18}", "Date", "BTC Flow", "USD Flow");
            println!("{}", "-".repeat(48));

            for date in &dates {
                let (btc, usd) = daily_totals[date];
                println!("{:<12} {:>15.2} {:>18.2}", date, btc, usd);
            }
            println!();
        }

        // Fund-level detail
        let mut fund_data: Vec<&EtfFlow> = fund_filtered.into_iter().collect();
        fund_data.sort_by(|a, b| b.date.cmp(&a.date).then(a.fund.cmp(&b.fund)));

        if let Some(ref fund_name) = fund_filter {
            println!("Fund: {}", fund_name);
        } else {
            println!("Fund Detail:");
        }
        println!(
            "{:<12} {:<10} {:>15} {:>18}",
            "Date", "Fund", "BTC Flow", "USD Flow"
        );
        println!("{}", "-".repeat(58));

        for flow in fund_data {
            println!(
                "{:<12} {:<10} {:>15.2} {:>18.2}",
                flow.date, flow.fund, flow.net_flow_btc, flow.net_flow_usd
            );
        }
    }

    Ok(())
}
