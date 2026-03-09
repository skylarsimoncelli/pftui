use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;

use crate::db::backend::BackendConnection;
use crate::db::allocations::get_unique_allocation_symbols;
use crate::db::correlation_snapshots;
use crate::db::price_history::get_history;
use crate::db::transactions::get_unique_symbols;
use crate::indicators::correlation::compute_rolling_correlation;
use crate::models::asset::AssetCategory;

const WINDOWS: [usize; 3] = [7, 30, 90];
const BREAK_THRESHOLD: f64 = 0.30;
const ANCHORS: [&str; 6] = ["DX-Y.NYB", "^GSPC", "SPY", "GC=F", "SI=F", "BTC-USD"];

#[derive(Debug, Clone)]
pub struct PairCorrelation {
    pub symbol_a: String,
    pub symbol_b: String,
    pub corr_7d: Option<f64>,
    pub corr_30d: Option<f64>,
    pub corr_90d: Option<f64>,
    pub break_delta: Option<f64>,
}

pub fn run(
    backend: &BackendConnection,
    action: Option<&str>,
    value: Option<&str>,
    value2: Option<&str>,
    window: usize,
    period: Option<&str>,
    store: bool,
    limit: usize,
    json: bool,
) -> Result<()> {
    let Some(conn) = backend.sqlite_native() else {
        anyhow::bail!("correlations currently requires database_backend=sqlite");
    };
    if !WINDOWS.contains(&window) {
        anyhow::bail!("Invalid --window '{}'. Use 7, 30, or 90.", window);
    }

    match action.unwrap_or("compute") {
        "compute" => {
            let pairs = compute_pairs(conn, window, limit)?;
            if store {
                let period_tag = period.unwrap_or("30d");
                let stored = store_pairs(conn, &pairs, period_tag)?;
                if !json {
                    println!("Stored {} correlation snapshots ({})", stored, period_tag);
                }
            }
            print_pairs(conn, pairs, window, limit, json)
        }
        "history" => {
            let symbol_a = value.ok_or_else(|| anyhow::anyhow!("symbol A required"))?;
            let symbol_b = value2.ok_or_else(|| anyhow::anyhow!("symbol B required"))?;
            let rows = correlation_snapshots::get_history(conn, symbol_a, symbol_b, period, Some(limit))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "symbol_a": symbol_a,
                        "symbol_b": symbol_b,
                        "period": period,
                        "history": rows,
                    }))?
                );
            } else if rows.is_empty() {
                println!("No snapshot history for {}-{}", symbol_a, symbol_b);
            } else {
                println!("Correlation history {}-{}:", symbol_a, symbol_b);
                for r in rows {
                    println!("  {}  {}  {:+.3}", r.recorded_at, r.period, r.correlation);
                }
            }
            Ok(())
        }
        other => anyhow::bail!("Unknown correlations action '{}'. Valid: compute, history", other),
    }
}

pub fn compute_pairs(conn: &Connection, window: usize, limit: usize) -> Result<Vec<PairCorrelation>> {
    let held = collect_held_symbols(conn);
    if held.is_empty() {
        return Ok(Vec::new());
    }

    let mut candidates: HashSet<String> = held.clone();
    for anchor in ANCHORS {
        candidates.insert(anchor.to_string());
    }

    let history_limit = 180u32;
    let mut series_map: HashMap<String, Vec<f64>> = HashMap::new();
    for symbol in &candidates {
        if let Some((resolved, closes)) = load_closes_with_fallback(conn, symbol, history_limit) {
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
    Ok(pairs)
}

fn store_pairs(conn: &Connection, pairs: &[PairCorrelation], period: &str) -> Result<usize> {
    let mut n = 0usize;
    for p in pairs {
        let corr = match period {
            "7d" => p.corr_7d,
            "30d" => p.corr_30d,
            "90d" => p.corr_90d,
            _ => p.corr_30d,
        };
        if let Some(c) = corr {
            let _ = correlation_snapshots::store_snapshot(conn, &p.symbol_a, &p.symbol_b, c, period)?;
            n += 1;
        }
    }
    Ok(n)
}

pub fn compute_and_store_default_snapshots(conn: &Connection) -> Result<usize> {
    let held = collect_held_symbols(conn);
    if held.is_empty() {
        return Ok(0);
    }

    let macro_symbols = ["SPY", "DX-Y.NYB", "GC=F", "CL=F", "^VIX"];
    let mut stored = 0usize;

    for held_symbol in held {
        for macro_symbol in macro_symbols {
            if held_symbol == macro_symbol {
                continue;
            }
            let a = load_closes_with_fallback(conn, &held_symbol, 120).map(|(_, v)| v);
            let b = load_closes_with_fallback(conn, macro_symbol, 120).map(|(_, v)| v);
            let (series_a, series_b) = match (a, b) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };

            let min_len = series_a.len().min(series_b.len());
            if min_len < 8 {
                continue;
            }
            let aligned_a = &series_a[series_a.len() - min_len..];
            let aligned_b = &series_b[series_b.len() - min_len..];

            for (period, w) in [("7d", 7usize), ("30d", 30usize), ("90d", 90usize)] {
                if min_len < w + 1 {
                    continue;
                }
                if let Some(corr) = latest_corr(aligned_a, aligned_b, w) {
                    let _ = correlation_snapshots::store_snapshot(
                        conn,
                        &held_symbol,
                        macro_symbol,
                        corr,
                        period,
                    )?;
                    stored += 1;
                }
            }
        }
    }

    Ok(stored)
}

fn print_pairs(
    conn: &Connection,
    pairs: Vec<PairCorrelation>,
    window: usize,
    limit: usize,
    json: bool,
) -> Result<()> {
    let held = collect_held_symbols(conn);

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

fn collect_held_symbols(conn: &Connection) -> HashSet<String> {
    let mut held = HashSet::new();
    if let Ok(rows) = get_unique_symbols(conn) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                held.insert(sym);
            }
        }
    }
    if let Ok(rows) = get_unique_allocation_symbols(conn) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                held.insert(sym);
            }
        }
    }
    held
}

fn load_closes_with_fallback(
    conn: &Connection,
    symbol: &str,
    limit: u32,
) -> Option<(String, Vec<f64>)> {
    if let Some(closes) = load_closes(conn, symbol, limit) {
        return Some((symbol.to_string(), closes));
    }

    if symbol.ends_with("-USD") {
        let stripped = symbol.trim_end_matches("-USD");
        if let Some(closes) = load_closes(conn, stripped, limit) {
            return Some((stripped.to_string(), closes));
        }
    } else {
        let usd = format!("{}-USD", symbol);
        if let Some(closes) = load_closes(conn, &usd, limit) {
            return Some((usd, closes));
        }
    }

    None
}

fn load_closes(conn: &Connection, symbol: &str, limit: u32) -> Option<Vec<f64>> {
    let history = get_history(conn, symbol, limit).ok()?;
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
