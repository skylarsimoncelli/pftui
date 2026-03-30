//! `pftui analytics views` — structured per-analyst, per-asset directional views.
//!
//! Subcommands:
//!   set    — write/update an analyst's view on an asset (upsert)
//!   list   — list current views with optional analyst/asset filters
//!   matrix — full cross-analyst view matrix (rows=assets, columns=analysts)
//!   delete — remove an analyst's view on an asset

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::json;

use crate::db::analyst_views;
use crate::db::backend::BackendConnection;
use crate::models::asset::AssetCategory;

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

/// Portfolio-aware view matrix: includes held + watched + viewed assets.
pub fn portfolio_matrix(backend: &BackendConnection, json_output: bool) -> Result<()> {
    // Collect held symbols (from transactions + allocations)
    let mut symbols = BTreeSet::new();

    if let Ok(rows) = crate::db::transactions::get_unique_symbols_backend(backend) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                symbols.insert(sym.to_uppercase());
            }
        }
    }
    if let Ok(rows) = crate::db::allocations::get_unique_allocation_symbols_backend(backend) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                symbols.insert(sym.to_uppercase());
            }
        }
    }

    // Collect watchlist symbols
    if let Ok(rows) = crate::db::watchlist::get_watchlist_symbols_backend(backend) {
        for (sym, _cat) in rows {
            symbols.insert(sym.to_uppercase());
        }
    }

    let portfolio_symbols: Vec<String> = symbols.into_iter().collect();
    let matrix = analyst_views::get_portfolio_view_matrix_backend(backend, &portfolio_symbols)?;

    if json_output {
        // Enrich JSON with coverage stats
        let total_assets = matrix.len();
        let analysts = ["low", "medium", "high", "macro"];
        let total_cells = total_assets * analysts.len();
        let filled_cells: usize = matrix
            .iter()
            .map(|row| {
                analysts
                    .iter()
                    .filter(|a| row.views.iter().any(|v| v.analyst == **a))
                    .count()
            })
            .sum();
        let coverage_pct = if total_cells > 0 {
            (filled_cells as f64 / total_cells as f64 * 100.0).round() as u64
        } else {
            0
        };

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "portfolio_matrix": matrix,
                "total_assets": total_assets,
                "total_cells": total_cells,
                "filled_cells": filled_cells,
                "coverage_pct": coverage_pct,
            }))?
        );
    } else if matrix.is_empty() {
        println!("No held, watched, or viewed assets found.");
        println!("Add positions, watchlist items, or set views with `analytics views set`.");
    } else {
        // Header
        println!(
            "{:<10} {:<16} {:<16} {:<16} {:<16}",
            "Asset", "LOW", "MEDIUM", "HIGH", "MACRO"
        );
        println!("{}", "═".repeat(74));

        let analysts = ["low", "medium", "high", "macro"];
        let mut filled = 0usize;
        let total = matrix.len() * analysts.len();

        for row in &matrix {
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
                    filled += 1;
                } else {
                    cells.push("—".to_string());
                }
            }
            println!(
                "{:<10} {:<16} {:<16} {:<16} {:<16}",
                row.asset, cells[0], cells[1], cells[2], cells[3]
            );
        }

        let coverage = if total > 0 {
            (filled as f64 / total as f64 * 100.0).round() as u64
        } else {
            0
        };
        println!(
            "\n{} asset(s) | {}/{} cells filled ({}% coverage)",
            matrix.len(),
            filled,
            total,
            coverage
        );
    }
    Ok(())
}

/// Show how analyst views on an asset have evolved over time.
pub fn history(
    backend: &BackendConnection,
    asset: &str,
    analyst: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    if let Some(a) = analyst {
        analyst_views::validate_analyst(a)?;
    }
    let entries = analyst_views::get_view_history_backend(backend, asset, analyst, limit)?;

    if json_output {
        // Compute conviction drift summary per analyst
        let mut drift_map: std::collections::BTreeMap<
            String,
            Vec<&analyst_views::AnalystViewHistoryEntry>,
        > = std::collections::BTreeMap::new();
        for e in &entries {
            drift_map.entry(e.analyst.clone()).or_default().push(e);
        }

        let mut drift_summary = Vec::new();
        for (analyst_name, hist) in &drift_map {
            // hist is sorted DESC (newest first)
            let latest = hist.first();
            let oldest = hist.last();
            let flips: usize = hist
                .windows(2)
                .filter(|w| w[0].direction != w[1].direction)
                .count();

            if let (Some(latest), Some(oldest)) = (latest, oldest) {
                drift_summary.push(json!({
                    "analyst": analyst_name,
                    "entries": hist.len(),
                    "current_direction": latest.direction,
                    "current_conviction": latest.conviction,
                    "first_direction": oldest.direction,
                    "first_conviction": oldest.conviction,
                    "conviction_drift": latest.conviction - oldest.conviction,
                    "direction_flips": flips,
                }));
            }
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "asset": asset.to_uppercase(),
                "entries": entries.len(),
                "analyst_filter": analyst,
                "history": entries,
                "drift_summary": drift_summary,
            }))?
        );
    } else if entries.is_empty() {
        println!(
            "No view history for {}.",
            asset.to_uppercase()
        );
        if analyst.is_some() {
            println!("Try without --analyst filter.");
        }
    } else {
        println!(
            "View history for {} ({} entries):\n",
            asset.to_uppercase(),
            entries.len()
        );
        println!(
            "{:<8} {:<9} {:<6} {:<32} Recorded",
            "Analyst", "Direction", "Conv", "Reasoning"
        );
        println!("{}", "-".repeat(80));
        for e in &entries {
            let icon = match e.direction.as_str() {
                "bull" => "🐂",
                "bear" => "🐻",
                _ => "⚖️",
            };
            let sign = if e.conviction > 0 { "+" } else { "" };
            let reasoning_short = if e.reasoning_summary.len() > 30 {
                format!("{}…", &e.reasoning_summary[..29])
            } else {
                e.reasoning_summary.clone()
            };
            let recorded_short = e.recorded_at.get(..16).unwrap_or(&e.recorded_at);
            println!(
                "{:<8} {} {:<7} {}{:<4} {:<32} {}",
                e.analyst,
                icon,
                e.direction,
                sign,
                e.conviction,
                reasoning_short,
                recorded_short,
            );
        }
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
