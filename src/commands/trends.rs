use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::trends;

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    trend_name: Option<&str>,
    category: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    description: Option<&str>,
    signal: Option<&str>,
    status: Option<&str>,
    date: Option<&str>,
    impact: Option<&str>,
    source: Option<&str>,
    symbol: Option<&str>,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("trend name required"))?;
            let id = trends::add_trend_backend(
                backend,
                name,
                timeframe,
                direction,
                conviction,
                category,
                description,
                impact,
                signal,
            )?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Added trend #{} {}", id, name);
            }
        }
        "list" => {
            let rows = trends::list_trends_backend(backend, status, category)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "trends": rows }))?);
            } else {
                println!("Trends ({}):", rows.len());
                for r in rows {
                    println!(
                        "  {} [{}|{}|{}]",
                        r.name, r.timeframe, r.direction, r.conviction
                    );
                }
            }
        }
        "update" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("trend name required"))?;
            trends::update_trend_backend(backend, name, direction, conviction, description, signal, status)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "updated": name }))?);
            } else {
                println!("Updated trend {}", name);
            }
        }
        "evidence-add" => {
            let name = trend_name.ok_or_else(|| anyhow::anyhow!("--trend required"))?;
            let evid = value
                .or(description)
                .ok_or_else(|| anyhow::anyhow!("evidence text required (positional value or --description)"))?;
            let d = date
                .map(|x| x.to_string())
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
            let id = trends::add_evidence_by_name_backend(backend, name, &d, evid, impact, source)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Added trend evidence #{}", id);
            }
        }
        "evidence-list" => {
            let name = trend_name.or(value).ok_or_else(|| anyhow::anyhow!("trend name required"))?;
            let trend = trends::list_trends_backend(backend, None, None)?
                .into_iter()
                .find(|t| t.name == name)
                .ok_or_else(|| anyhow::anyhow!("trend not found"))?;
            let rows = trends::list_evidence_backend(backend, trend.id, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "evidence": rows }))?);
            } else {
                println!("Evidence {} ({}):", name, rows.len());
                for r in rows {
                    println!("  {} {} ({:?})", r.date, r.evidence, r.direction_impact);
                }
            }
        }
        "impact-add" => {
            let name = trend_name.or(value).ok_or_else(|| anyhow::anyhow!("trend name required"))?;
            let sym = symbol.ok_or_else(|| anyhow::anyhow!("--symbol required"))?;
            let imp = impact.ok_or_else(|| anyhow::anyhow!("--impact required"))?;
            let id = trends::add_asset_impact_by_name_backend(backend, name, sym, imp, mechanism, timeframe)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Added trend asset impact #{}", id);
            }
        }
        "impact-list" => {
            if let Some(sym) = symbol {
                let rows = trends::get_impacts_for_symbol_backend(backend, sym)?;
                if json_output {
                    println!("{}", serde_json::to_string_pretty(&json!({ "impacts": rows }))?);
                } else {
                    println!("Impacts for {} ({}):", sym, rows.len());
                    for (t, i) in rows {
                        println!("  {} -> {} ({:?})", t.name, i.impact, i.mechanism);
                    }
                }
            } else {
                let name = trend_name.or(value).ok_or_else(|| anyhow::anyhow!("trend name required"))?;
                let trend = trends::list_trends_backend(backend, None, None)?
                    .into_iter()
                    .find(|t| t.name == name)
                    .ok_or_else(|| anyhow::anyhow!("trend not found"))?;
                let rows = trends::list_asset_impacts_backend(backend, trend.id)?;
                if json_output {
                    println!("{}", serde_json::to_string_pretty(&json!({ "impacts": rows }))?);
                } else {
                    println!("Trend impacts {} ({}):", name, rows.len());
                    for i in rows {
                        println!("  {} {} {:?}", i.symbol, i.impact, i.mechanism);
                    }
                }
            }
        }
        "dashboard" => {
            let trends_list = trends::list_trends_backend(backend, Some("active"), None)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "trends": trends_list }))?);
            } else {
                println!("Trends Dashboard");
                println!("════════════════════════════════════════");
                for t in trends_list {
                    println!("  {} [{}|{}]", t.name, t.direction, t.conviction);
                    if let Some(sig) = t.key_signal { println!("    signal: {}", sig); }
                }
            }
        }
        _ => bail!("unknown trends action '{}'. See --help", action),
    }

    Ok(())
}
