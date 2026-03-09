use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;

use crate::db::allocations::get_unique_allocation_symbols_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_history::get_history_backend;
use crate::db::transactions::get_unique_symbols_backend;
use crate::indicators::correlation::compute_rolling_correlation;
use crate::models::asset::AssetCategory;

const WINDOWS: [usize; 3] = [7, 30, 90];
const BREAK_THRESHOLD: f64 = 0.30;
const ANCHORS: [&str; 6] = ["DX-Y.NYB", "^GSPC", "SPY", "GC=F", "SI=F", "BTC-USD"];

#[derive(Debug, Clone)]
struct PairCorrelation {
    symbol_a: String,
    symbol_b: String,
    corr_7d: Option<f64>,
    corr_30d: Option<f64>,
    corr_90d: Option<f64>,
    break_delta: Option<f64>,
}

pub fn run(
    backend: &BackendConnection,
    _conn: &Connection,
    window: usize,
    limit: usize,
    json: bool,
) -> Result<()> {
    if !WINDOWS.contains(&window) {
        anyhow::bail!("Invalid --window '{}'. Use 7, 30, or 90.", window);
    }

    let held = collect_held_symbols(backend);
    if held.is_empty() {
        println!("No held symbols found. Add positions first.");
        return Ok(());
    }

    let mut candidates: HashSet<String> = held.clone();
    for anchor in ANCHORS {
        candidates.insert(anchor.to_string());
    }

    let history_limit = 180u32;
    let mut series_map: HashMap<String, Vec<f64>> = HashMap::new();
    for symbol in &candidates {
        if let Some((resolved, closes)) = load_closes_with_fallback(backend, symbol, history_limit) {
            if closes.len() >= 91 {
                series_map.entry(symbol.clone()).or_insert(closes.clone());
                series_map.entry(resolved).or_insert(closes);
            }
        }
    }

    let mut pairs = Vec::new();
    let mut symbols: Vec<String> = candidates.into_iter().collect();
    symbols.sort();

    for i in 0..symbols.len() {
        for j in (i + 1)..symbols.len() {
            let a = &symbols[i];
            let b = &symbols[j];
            // Keep at least one held asset in every output pair.
            if !held.contains(a) && !held.contains(b) {
                continue;
            }
            let series_a = match series_map.get(a) {
                Some(v) => v,
                None => continue,
            };
            let series_b = match series_map.get(b) {
                Some(v) => v,
                None => continue,
            };
            let min_len = series_a.len().min(series_b.len());
            if min_len < 91 {
                continue;
            }
            let aligned_a = &series_a[series_a.len() - min_len..];
            let aligned_b = &series_b[series_b.len() - min_len..];
            let corr_7d = latest_corr(aligned_a, aligned_b, 7);
            let corr_30d = latest_corr(aligned_a, aligned_b, 30);
            let corr_90d = latest_corr(aligned_a, aligned_b, 90);
            let break_delta = match (corr_7d, corr_90d) {
                (Some(c7), Some(c90)) if (c7 - c90).abs() >= BREAK_THRESHOLD => Some(c7 - c90),
                _ => None,
            };
            if corr_7d.is_some() || corr_30d.is_some() || corr_90d.is_some() {
                pairs.push(PairCorrelation {
                    symbol_a: a.clone(),
                    symbol_b: b.clone(),
                    corr_7d,
                    corr_30d,
                    corr_90d,
                    break_delta,
                });
            }
        }
    }

    let score = |p: &PairCorrelation| match window {
        7 => p.corr_7d.map(|v| v.abs()).unwrap_or(0.0),
        30 => p.corr_30d.map(|v| v.abs()).unwrap_or(0.0),
        90 => p.corr_90d.map(|v| v.abs()).unwrap_or(0.0),
        _ => 0.0,
    };
    pairs.sort_by(|a, b| score(b).partial_cmp(&score(a)).unwrap_or(std::cmp::Ordering::Equal));
    pairs.truncate(limit.max(1));

    if json {
        let output = serde_json::json!({
            "window": window,
            "limit": limit,
            "held_symbols": held.into_iter().collect::<Vec<_>>(),
            "pairs": pairs.iter().map(|p| serde_json::json!({
                "symbol_a": p.symbol_a,
                "symbol_b": p.symbol_b,
                "corr_7d": p.corr_7d,
                "corr_30d": p.corr_30d,
                "corr_90d": p.corr_90d,
                "break_delta": p.break_delta,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if pairs.is_empty() {
        println!("No correlation pairs available yet. Run `pftui refresh` to build more history.");
        return Ok(());
    }

    println!("Rolling Correlations (sorted by {}d absolute correlation)", window);
    println!();
    println!(
        "{:<22} {:>8} {:>8} {:>8} {:>10}",
        "Pair", "7d", "30d", "90d", "Break Δ"
    );
    println!("{}", "─".repeat(62));
    for p in &pairs {
        let pair = format!("{}-{}", p.symbol_a, p.symbol_b);
        println!(
            "{:<22} {:>8} {:>8} {:>8} {:>10}",
            truncate(&pair, 22),
            fmt_opt(p.corr_7d),
            fmt_opt(p.corr_30d),
            fmt_opt(p.corr_90d),
            fmt_opt(p.break_delta),
        );
    }

    Ok(())
}

fn collect_held_symbols(backend: &BackendConnection) -> HashSet<String> {
    let mut held = HashSet::new();
    if let Ok(rows) = get_unique_symbols_backend(backend) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                held.insert(sym);
            }
        }
    }
    if let Ok(rows) = get_unique_allocation_symbols_backend(backend) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                held.insert(sym);
            }
        }
    }
    held
}

fn load_closes_with_fallback(
    backend: &BackendConnection,
    symbol: &str,
    limit: u32,
) -> Option<(String, Vec<f64>)> {
    if let Some(closes) = load_closes(backend, symbol, limit) {
        return Some((symbol.to_string(), closes));
    }

    // Common crypto fallback: BTC <-> BTC-USD
    if symbol.ends_with("-USD") {
        let stripped = symbol.trim_end_matches("-USD");
        if let Some(closes) = load_closes(backend, stripped, limit) {
            return Some((stripped.to_string(), closes));
        }
    } else {
        let usd = format!("{}-USD", symbol);
        if let Some(closes) = load_closes(backend, &usd, limit) {
            return Some((usd, closes));
        }
    }

    None
}

fn load_closes(backend: &BackendConnection, symbol: &str, limit: u32) -> Option<Vec<f64>> {
    let history = get_history_backend(backend, symbol, limit).ok()?;
    let closes: Vec<f64> = history
        .into_iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .filter(|v| *v > 0.0)
        .collect();
    if closes.len() < 2 {
        None
    } else {
        Some(closes)
    }
}

fn latest_corr(series_a: &[f64], series_b: &[f64], window: usize) -> Option<f64> {
    if series_a.len() != series_b.len() || series_a.len() < window + 1 {
        return None;
    }
    compute_rolling_correlation(series_a, series_b, window)
        .into_iter()
        .rev()
        .flatten()
        .next()
}

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{:+.2}", x),
        None => "---".to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max > 3 {
        format!("{}...", &s[..max - 3])
    } else {
        s[..max].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_corr_detects_positive_relationship() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let b = vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0];
        let corr = latest_corr(&a, &b, 3).unwrap();
        assert!(corr > 0.9);
    }

    #[test]
    fn fmt_opt_formats_missing_and_present() {
        assert_eq!(fmt_opt(None), "---");
        assert_eq!(fmt_opt(Some(0.1234)), "+0.12");
    }
}
