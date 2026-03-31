use crate::db::backend::BackendConnection;
use crate::db::trends;
use anyhow::{bail, Result};
use serde_json::json;
use std::collections::HashMap;

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
            let trends_list = trends::list_trends_backend(backend, status, category)?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "trends": trends_list }))?);
            } else {
                if trends_list.is_empty() {
                    println!("No trends found.");
                    return Ok(());
                }

                println!(
                    "{:<40} {:<10} {:<12} {:<10} {:<12} {:<10}",
                    "Name", "Timeframe", "Direction", "Conviction", "Category", "Status"
                );
                println!("{}", "─".repeat(100));

                for t in trends_list {
                    println!(
                        "{:<40} {:<10} {:<12} {:<10} {:<12} {:<10}",
                        truncate(&t.name, 38),
                        t.timeframe,
                        t.direction,
                        t.conviction,
                        t.category.as_deref().unwrap_or("—"),
                        t.status
                    );
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

/// Enriched trend list with evidence summaries inline.
/// Called directly from main.rs when `--with-evidence` is set.
#[allow(clippy::too_many_arguments)]
pub fn run_list(
    backend: &BackendConnection,
    status: Option<&str>,
    category: Option<&str>,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    with_evidence: bool,
    _limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut trends_list = trends::list_trends_backend(backend, status, category)?;

    // Client-side filters for fields not supported by the DB query
    if let Some(tf) = timeframe {
        trends_list.retain(|t| t.timeframe.eq_ignore_ascii_case(tf));
    }
    if let Some(dir) = direction {
        trends_list.retain(|t| t.direction.eq_ignore_ascii_case(dir));
    }
    if let Some(conv) = conviction {
        trends_list.retain(|t| t.conviction.eq_ignore_ascii_case(conv));
    }

    if trends_list.is_empty() {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&json!({ "trends": [] }))?);
        } else {
            println!("No trends found.");
        }
        return Ok(());
    }

    if !with_evidence {
        // Delegate to the basic list in run()
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "trends": trends_list }))?
            );
        } else {
            println!(
                "{:<40} {:<10} {:<12} {:<10} {:<12} {:<10}",
                "Name", "Timeframe", "Direction", "Conviction", "Category", "Status"
            );
            println!("{}", "─".repeat(100));
            for t in trends_list {
                println!(
                    "{:<40} {:<10} {:<12} {:<10} {:<12} {:<10}",
                    truncate(&t.name, 38),
                    t.timeframe,
                    t.direction,
                    t.conviction,
                    t.category.as_deref().unwrap_or("—"),
                    t.status
                );
            }
        }
        return Ok(());
    }

    // Fetch evidence summaries and build a lookup map
    let summaries = trends::get_evidence_summaries_backend(backend)?;
    let summary_map: HashMap<i64, &trends::TrendEvidenceSummary> =
        summaries.iter().map(|s| (s.trend_id, s)).collect();

    if json_output {
        let enriched: Vec<serde_json::Value> = trends_list
            .iter()
            .map(|t| {
                let summary = summary_map.get(&t.id);
                let mut val = serde_json::to_value(t).unwrap_or_default();
                if let Some(s) = summary {
                    val["evidence_count"] = json!(s.evidence_count);
                    val["latest_evidence_date"] = json!(s.latest_date);
                    val["latest_evidence"] = json!(s.latest_evidence);
                    val["latest_evidence_impact"] = json!(s.latest_direction_impact);
                    val["strengthens_count"] = json!(s.strengthens_count);
                    val["weakens_count"] = json!(s.weakens_count);
                } else {
                    val["evidence_count"] = json!(0);
                    val["latest_evidence_date"] = json!(null);
                    val["latest_evidence"] = json!(null);
                    val["latest_evidence_impact"] = json!(null);
                    val["strengthens_count"] = json!(0);
                    val["weakens_count"] = json!(0);
                }
                val
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "trends": enriched }))?
        );
    } else {
        println!(
            "{name:<32} {tf:<10} {dir:<12} {conv:<10} {evid:<6} {ratio:<8} {date:<12} Latest Evidence",
            name = "Name",
            tf = "Timeframe",
            dir = "Direction",
            conv = "Conviction",
            evid = "Evid.",
            ratio = "↑/↓",
            date = "Last Date"
        );
        println!("{}", "─".repeat(130));

        for t in &trends_list {
            let summary = summary_map.get(&t.id);
            let evidence_count = summary.map_or(0, |s| s.evidence_count);
            let strengthens = summary.map_or(0, |s| s.strengthens_count);
            let weakens = summary.map_or(0, |s| s.weakens_count);
            let latest_date = summary
                .and_then(|s| s.latest_date.as_deref())
                .unwrap_or("—");
            let latest_ev = summary
                .and_then(|s| s.latest_evidence.as_deref())
                .unwrap_or("—");

            println!(
                "{:<32} {:<10} {:<12} {:<10} {:<6} {:<8} {:<12} {}",
                truncate(&t.name, 30),
                t.timeframe,
                t.direction,
                t.conviction,
                evidence_count,
                format!("{}↑/{}↓", strengthens, weakens),
                truncate(latest_date, 10),
                truncate(latest_ev, 50),
            );
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max - 3])
    } else {
        s.to_string()
    }
}
