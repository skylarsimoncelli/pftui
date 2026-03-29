//! `pftui analytics views` — structured per-analyst, per-asset directional views.
//!
//! Subcommands:
//!   set    — write/update an analyst's view on an asset (upsert)
//!   list   — list current views with optional analyst/asset filters
//!   matrix — full cross-analyst view matrix (rows=assets, columns=analysts)
//!   delete — remove an analyst's view on an asset

use anyhow::Result;
use serde_json::json;

use crate::db::analyst_views;
use crate::db::backend::BackendConnection;

/// Set (upsert) an analyst's view on an asset.
#[allow(clippy::too_many_arguments)]
pub fn set(
    backend: &BackendConnection,
    analyst: &str,
    asset: &str,
    direction: &str,
    conviction: i64,
    reasoning: &str,
    evidence: Option<&str>,
    blind_spots: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let id = analyst_views::upsert_view_backend(
        backend,
        analyst,
        asset,
        direction,
        conviction,
        reasoning,
        evidence,
        blind_spots,
    )?;

    if json_output {
        let view = analyst_views::get_view_backend(backend, analyst, asset)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "view_set",
                "id": id,
                "analyst": analyst,
                "asset": asset,
                "view": view,
            }))?
        );
    } else {
        let icon = match direction {
            "bull" => "🐂",
            "bear" => "🐻",
            _ => "⚖️",
        };
        let sign = if conviction > 0 { "+" } else { "" };
        println!(
            "Set {}'s view on {}: {} {} ({}{})",
            analyst.to_uppercase(),
            asset.to_uppercase(),
            icon,
            direction,
            sign,
            conviction
        );
    }
    Ok(())
}

/// List analyst views with optional filters.
pub fn list(
    backend: &BackendConnection,
    analyst: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let items = analyst_views::list_views_backend(backend, analyst, asset, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("No analyst views found.");
        if analyst.is_some() || asset.is_some() {
            println!("Try without filters, or set views with `analytics views set`.");
        }
    } else {
        println!(
            "{:<8} {:<8} {:<9} {:<6} {:<20} Updated",
            "Analyst", "Asset", "Direction", "Conv", "Reasoning"
        );
        println!("{}", "-".repeat(75));
        for v in &items {
            let icon = match v.direction.as_str() {
                "bull" => "🐂",
                "bear" => "🐻",
                _ => "⚖️",
            };
            let sign = if v.conviction > 0 { "+" } else { "" };
            let reasoning_short = if v.reasoning_summary.len() > 18 {
                format!("{}…", &v.reasoning_summary[..17])
            } else {
                v.reasoning_summary.clone()
            };
            let updated_short = v.updated_at.get(..16).unwrap_or(&v.updated_at);
            println!(
                "{:<8} {:<8} {} {:<7} {}{:<4} {:<20} {}",
                v.analyst,
                v.asset,
                icon,
                v.direction,
                sign,
                v.conviction,
                reasoning_short,
                updated_short,
            );
        }
        println!("\n{} view(s)", items.len());
    }
    Ok(())
}

/// Show the full view matrix: rows=assets, columns=analysts.
pub fn matrix(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let matrix = analyst_views::get_view_matrix_backend(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&matrix)?);
    } else if matrix.is_empty() {
        println!("No analyst views found. Set views with `analytics views set`.");
    } else {
        // Header
        println!(
            "{:<10} {:<16} {:<16} {:<16} {:<16}",
            "Asset", "LOW", "MEDIUM", "HIGH", "MACRO"
        );
        println!("{}", "═".repeat(74));

        for row in &matrix {
            let analysts = ["low", "medium", "high", "macro"];
            let mut cells: Vec<String> = Vec::new();
            for a in &analysts {
                if let Some(v) = row.views.iter().find(|v| v.analyst == *a) {
                    let icon = match v.direction.as_str() {
                        "bull" => "🐂",
                        "bear" => "🐻",
                        _ => "⚖️",
                    };
                    let sign = if v.conviction > 0 { "+" } else { "" };
                    cells.push(format!("{} {}{}", icon, sign, v.conviction));
                } else {
                    cells.push("—".to_string());
                }
            }
            println!(
                "{:<10} {:<16} {:<16} {:<16} {:<16}",
                row.asset, cells[0], cells[1], cells[2], cells[3]
            );
        }
        println!("\n{} asset(s) with views", matrix.len());
    }
    Ok(())
}

/// Delete an analyst's view on an asset.
pub fn delete(
    backend: &BackendConnection,
    analyst: &str,
    asset: &str,
    json_output: bool,
) -> Result<()> {
    analyst_views::validate_analyst(analyst)?;
    let deleted = analyst_views::delete_view_backend(backend, analyst, asset)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "view_deleted",
                "analyst": analyst,
                "asset": asset,
                "deleted": deleted,
            }))?
        );
    } else if deleted {
        println!(
            "Deleted {}'s view on {}.",
            analyst.to_uppercase(),
            asset.to_uppercase()
        );
    } else {
        println!(
            "No view found for {} on {}.",
            analyst.to_uppercase(),
            asset.to_uppercase()
        );
    }
    Ok(())
}
