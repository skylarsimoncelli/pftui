use crate::db::backend::BackendConnection;
use crate::db::trends;
use anyhow::{bail, Result};
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    category: Option<&str>,
    description: Option<&str>,
    asset_impact: Option<&str>,
    key_signal: Option<&str>,
    status: Option<&str>,
    date: Option<&str>,
    evidence: Option<&str>,
    direction_impact: Option<&str>,
    source: Option<&str>,
    symbol: Option<&str>,
    impact: Option<&str>,
    mechanism: Option<&str>,
    impact_timeframe: Option<&str>,
    limit: Option<usize>,
    verbose: bool,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("Trend name required"))?;
            let tf = timeframe.unwrap_or("high");
            let dir = direction.unwrap_or("neutral");
            let conv = conviction.unwrap_or("medium");

            let trend_id = trends::add_trend_backend(
                backend, name, tf, dir, conv, category, description, asset_impact, key_signal,
            )?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": trend_id, "name": name }))?);
            } else {
                println!("Added trend #{}: {}", trend_id, name);
            }
            Ok(())
        }
        "list" => {
            let trends_list = trends::list_trends_filtered_backend(
                backend, status, category, timeframe, direction, conviction, limit,
            )?;

            if json_output {
                let mut enriched = Vec::new();
                for t in &trends_list {
                    let evidence_list = trends::list_evidence_backend(backend, t.id, None).unwrap_or_default();
                    let impacts = trends::list_asset_impacts_backend(backend, t.id).unwrap_or_default();
                    let evidence_count = evidence_list.len();
                    let latest_evidence = evidence_list.first().map(|e| e.date.clone());
                    let recent_evidence: Vec<_> = evidence_list.into_iter().take(3).collect();

                    let bullish: Vec<_> = impacts.iter().filter(|i| i.impact == "bullish").map(|i| i.symbol.as_str()).collect();
                    let bearish: Vec<_> = impacts.iter().filter(|i| i.impact == "bearish").map(|i| i.symbol.as_str()).collect();

                    enriched.push(json!({
                        "trend": t,
                        "evidence_count": evidence_count,
                        "latest_evidence_date": latest_evidence,
                        "recent_evidence": recent_evidence,
                        "asset_impacts": {
                            "bullish": bullish,
                            "bearish": bearish,
                            "total": impacts.len(),
                        },
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&json!({ "trends": enriched }))?);
            } else {
                if trends_list.is_empty() {
                    println!("No trends found.");
                    return Ok(());
                }

                if verbose {
                    // Enriched output with evidence and asset impacts inline
                    for (i, t) in trends_list.iter().enumerate() {
                        if i > 0 {
                            println!();
                        }
                        let direction_symbol = match t.direction.as_str() {
                            "accelerating" => "▲",
                            "stable" => "→",
                            "decelerating" => "▽",
                            "reversing" => "◀",
                            _ => "•",
                        };

                        println!(
                            "{} {} [{}/{}] — {} ({})",
                            direction_symbol,
                            t.name,
                            t.timeframe,
                            t.category.as_deref().unwrap_or("—"),
                            t.direction.to_uppercase(),
                            t.conviction
                        );

                        if let Some(desc) = &t.description {
                            println!("  {}", desc);
                        }

                        if let Some(sig) = &t.key_signal {
                            println!("  Key signal: {}", sig);
                        }

                        let evidence_list = trends::list_evidence_backend(backend, t.id, Some(3)).unwrap_or_default();
                        if !evidence_list.is_empty() {
                            println!("  Evidence ({} total):", trends::count_evidence_backend(backend, t.id).unwrap_or(evidence_list.len()));
                            for e in evidence_list {
                                let impact_mark = match e.direction_impact.as_deref() {
                                    Some("strengthens") => "↑",
                                    Some("weakens") => "↓",
                                    _ => "•",
                                };
                                println!("    {} [{}] {}", impact_mark, e.date, truncate(&e.evidence, 60));
                            }
                        }

                        let impacts = trends::list_asset_impacts_backend(backend, t.id).unwrap_or_default();
                        if !impacts.is_empty() {
                            let bullish: Vec<_> = impacts.iter().filter(|i| i.impact == "bullish").map(|i| i.symbol.as_str()).collect();
                            let bearish: Vec<_> = impacts.iter().filter(|i| i.impact == "bearish").map(|i| i.symbol.as_str()).collect();

                            if !bullish.is_empty() {
                                println!("  Bullish: {}", bullish.join(", "));
                            }
                            if !bearish.is_empty() {
                                println!("  Bearish: {}", bearish.join(", "));
                            }
                        }
                    }
                } else {
                    // Compact table with evidence summary columns
                    println!(
                        "{:<36} {:<8} {:<12} {:<10} {:<10} {:<8} {:<12} Impacts",
                        "Name", "TF", "Direction", "Conviction", "Category", "Evid#", "Last Evid"
                    );
                    println!("{}", "─".repeat(110));

                    for t in &trends_list {
                        let evidence_count = trends::count_evidence_backend(backend, t.id).unwrap_or(0);
                        let evidence_list = trends::list_evidence_backend(backend, t.id, Some(1)).unwrap_or_default();
                        let latest_date = evidence_list.first().map(|e| e.date.as_str()).unwrap_or("—");

                        let impacts = trends::list_asset_impacts_backend(backend, t.id).unwrap_or_default();
                        let impact_summary = if impacts.is_empty() {
                            "—".to_string()
                        } else {
                            let bullish_count = impacts.iter().filter(|i| i.impact == "bullish").count();
                            let bearish_count = impacts.iter().filter(|i| i.impact == "bearish").count();
                            format!("↑{} ↓{}", bullish_count, bearish_count)
                        };

                        println!(
                            "{:<36} {:<8} {:<12} {:<10} {:<10} {:<8} {:<12} {}",
                            truncate(&t.name, 34),
                            t.timeframe,
                            t.direction,
                            t.conviction,
                            t.category.as_deref().unwrap_or("—"),
                            evidence_count,
                            latest_date,
                            impact_summary
                        );
                    }
                }
            }
            Ok(())
        }
        "update" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("Trend name required for update"))?;
            trends::update_trend_backend(backend, name, direction, conviction, description, key_signal, status)?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "status": "updated", "name": name }))?);
            } else {
                println!("Updated trend: {}", name);
            }
            Ok(())
        }
        "evidence-add" => {
            let trend_id_val = id.ok_or_else(|| anyhow::anyhow!("--id required for evidence-add"))?;
            let ev = evidence.ok_or_else(|| anyhow::anyhow!("--evidence required"))?;
            let default_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let dt = date.unwrap_or(&default_date);

            let evidence_id = trends::add_evidence_backend(
                backend,
                trend_id_val,
                dt,
                ev,
                direction_impact,
                source,
            )?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": evidence_id, "trend_id": trend_id_val }))?);
            } else {
                println!("Added evidence #{} to trend #{}", evidence_id, trend_id_val);
            }
            Ok(())
        }
        "evidence-list" => {
            let trend_id_val = id.ok_or_else(|| anyhow::anyhow!("--id required for evidence-list"))?;
            let evidence_list = trends::list_evidence_backend(backend, trend_id_val, limit)?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "evidence": evidence_list }))?);
            } else {
                if evidence_list.is_empty() {
                    println!("No evidence found for trend #{}.", trend_id_val);
                    return Ok(());
                }

                println!(
                    "{:<12} {:<50} {:<15} {:<15}",
                    "Date", "Evidence", "Impact", "Source"
                );
                println!("{}", "─".repeat(95));

                for e in evidence_list {
                    println!(
                        "{:<12} {:<50} {:<15} {:<15}",
                        e.date,
                        truncate(&e.evidence, 48),
                        e.direction_impact.as_deref().unwrap_or("—"),
                        e.source.as_deref().unwrap_or("—")
                    );
                }
            }
            Ok(())
        }
        "impact-add" => {
            let trend_id_val = id.ok_or_else(|| anyhow::anyhow!("--id required for impact-add"))?;
            let sym = symbol.ok_or_else(|| anyhow::anyhow!("--symbol required"))?;
            let imp = impact.ok_or_else(|| anyhow::anyhow!("--impact required (bullish/bearish/neutral)"))?;

            let impact_id = trends::add_asset_impact_backend(
                backend,
                trend_id_val,
                sym,
                imp,
                mechanism,
                impact_timeframe,
            )?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": impact_id, "trend_id": trend_id_val, "symbol": sym }))?);
            } else {
                println!("Added asset impact #{} (trend #{}, symbol {})", impact_id, trend_id_val, sym);
            }
            Ok(())
        }
        "impact-list" => {
            let trend_id_val = id.ok_or_else(|| anyhow::anyhow!("--id required for impact-list"))?;
            let impacts = trends::list_asset_impacts_backend(backend, trend_id_val)?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "impacts": impacts }))?);
            } else {
                if impacts.is_empty() {
                    println!("No asset impacts found for trend #{}.", trend_id_val);
                    return Ok(());
                }

                println!(
                    "{:<10} {:<10} {:<40} {:<15}",
                    "Symbol", "Impact", "Mechanism", "Timeframe"
                );
                println!("{}", "─".repeat(80));

                for i in impacts {
                    println!(
                        "{:<10} {:<10} {:<40} {:<15}",
                        i.symbol,
                        i.impact,
                        i.mechanism.as_deref().unwrap_or("—"),
                        i.timeframe.as_deref().unwrap_or("—")
                    );
                }
            }
            Ok(())
        }
        "dashboard" => {
            let trends_list = trends::list_trends_backend(backend, Some("active"), None)?;

            if json_output {
                let mut dashboard_data = Vec::new();
                for t in &trends_list {
                    let evidence_list = trends::list_evidence_backend(backend, t.id, Some(3))?;
                    let impacts = trends::list_asset_impacts_backend(backend, t.id)?;
                    dashboard_data.push(json!({
                        "trend": t,
                        "recent_evidence": evidence_list,
                        "asset_impacts": impacts,
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&json!({ "dashboard": dashboard_data }))?);
            } else {
                if trends_list.is_empty() {
                    println!("No active trends.");
                    return Ok(());
                }

                println!("HIGH-Timeframe Trends Dashboard");
                println!("{}", "═".repeat(80));

                for t in &trends_list {
                    let direction_symbol = match t.direction.as_str() {
                        "accelerating" => "▲",
                        "stable" => "→",
                        "decelerating" => "▽",
                        "reversing" => "◀",
                        _ => "•",
                    };

                    println!(
                        "\n{} {} — {} ({})",
                        direction_symbol,
                        t.name,
                        t.direction.to_uppercase(),
                        t.conviction
                    );

                    if let Some(desc) = &t.description {
                        println!("  {}", desc);
                    }

                    if let Some(sig) = &t.key_signal {
                        println!("  Key signal: {}", sig);
                    }

                    let evidence_list = trends::list_evidence_backend(backend, t.id, Some(3))?;
                    if !evidence_list.is_empty() {
                        println!("  Recent evidence:");
                        for e in evidence_list {
                            let impact_mark = match e.direction_impact.as_deref() {
                                Some("strengthens") => "↑",
                                Some("weakens") => "↓",
                                _ => "•",
                            };
                            println!("    {} [{}] {}", impact_mark, e.date, truncate(&e.evidence, 60));
                        }
                    }

                    let impacts = trends::list_asset_impacts_backend(backend, t.id)?;
                    if !impacts.is_empty() {
                        let bullish: Vec<_> = impacts.iter().filter(|i| i.impact == "bullish").collect();
                        let bearish: Vec<_> = impacts.iter().filter(|i| i.impact == "bearish").collect();

                        if !bullish.is_empty() {
                            let symbols: Vec<_> = bullish.iter().map(|i| i.symbol.as_str()).collect();
                            println!("  Bullish: {}", symbols.join(", "));
                        }
                        if !bearish.is_empty() {
                            let symbols: Vec<_> = bearish.iter().map(|i| i.symbol.as_str()).collect();
                            println!("  Bearish: {}", symbols.join(", "));
                        }
                    }
                }
            }
            Ok(())
        }
        _ => bail!("Unknown action: {}. Use add, list, update, evidence-add, evidence-list, impact-add, impact-list, dashboard", action),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max - 3])
    } else {
        s.to_string()
    }
}
