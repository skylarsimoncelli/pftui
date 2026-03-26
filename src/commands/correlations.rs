use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;

use crate::alerts::AlertKind;
use crate::db::alerts as db_alerts;
use crate::db::allocations::get_unique_allocation_symbols;
use crate::db::allocations::get_unique_allocation_symbols_backend;
use crate::db::backend::BackendConnection;
use crate::db::correlation_snapshots;
use crate::db::price_history::get_history;
use crate::db::price_history::get_history_backend;
use crate::db::transactions::get_unique_symbols;
use crate::db::transactions::get_unique_symbols_backend;
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

#[allow(clippy::too_many_arguments)]
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
    if !WINDOWS.contains(&window) {
        anyhow::bail!("Invalid --window '{}'. Use 7, 30, or 90.", window);
    }

    match action.unwrap_or("compute") {
        "compute" => {
            let pairs = compute_pairs_backend(backend, window, limit)?;
            if store {
                let period_tag = period.unwrap_or("30d");
                let stored = store_pairs_backend(backend, &pairs, period_tag)?;
                if !json {
                    println!("Stored {} correlation snapshots ({})", stored, period_tag);
                }
            }
            let held = collect_held_symbols_backend(backend);
            print_pairs(held, pairs, window, limit, json)
        }
        "history" => {
            let symbol_a = value.ok_or_else(|| anyhow::anyhow!("symbol A required"))?;
            let symbol_b = value2.ok_or_else(|| anyhow::anyhow!("symbol B required"))?;
            let rows = correlation_snapshots::get_history_backend(
                backend,
                symbol_a,
                symbol_b,
                period,
                Some(limit),
            )?;
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
        "latest" => {
            let mut rows = correlation_snapshots::list_current_backend(backend, period)?;
            rows.sort_by(|a, b| {
                b.correlation
                    .abs()
                    .partial_cmp(&a.correlation.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            rows.truncate(limit.max(1));
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "period": period,
                        "snapshots": rows,
                        "count": rows.len(),
                    }))?
                );
            } else if rows.is_empty() {
                println!("No stored correlation snapshots found. Run `pftui analytics correlations compute --store` first.");
            } else {
                println!("Latest correlation snapshots:");
                for row in rows {
                    println!(
                        "  {:<8} {:<10} {:<10} {:+.3} ({})",
                        row.period, row.symbol_a, row.symbol_b, row.correlation, row.recorded_at
                    );
                }
            }
            Ok(())
        }
        other => anyhow::bail!(
            "Unknown correlations action '{}'. Valid: compute, history, latest",
            other
        ),
    }
}

pub fn compute_pairs_backend(
    backend: &BackendConnection,
    window: usize,
    limit: usize,
) -> Result<Vec<PairCorrelation>> {
    let held = collect_held_symbols_backend(backend);
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
        if let Some((resolved, closes)) =
            load_closes_with_fallback_backend(backend, symbol, history_limit)
        {
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
    pairs.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pairs.truncate(limit.max(1));
    Ok(pairs)
}

#[allow(dead_code)]
pub fn compute_pairs(
    conn: &Connection,
    window: usize,
    limit: usize,
) -> Result<Vec<PairCorrelation>> {
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
    pairs.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pairs.truncate(limit.max(1));
    Ok(pairs)
}

#[allow(dead_code)]
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
            let _ =
                correlation_snapshots::store_snapshot(conn, &p.symbol_a, &p.symbol_b, c, period)?;
            n += 1;
        }
    }
    Ok(n)
}

fn store_pairs_backend(
    backend: &BackendConnection,
    pairs: &[PairCorrelation],
    period: &str,
) -> Result<usize> {
    let mut n = 0usize;
    for p in pairs {
        let corr = match period {
            "7d" => p.corr_7d,
            "30d" => p.corr_30d,
            "90d" => p.corr_90d,
            _ => p.corr_30d,
        };
        if let Some(c) = corr {
            let _ = correlation_snapshots::store_snapshot_backend(
                backend,
                &p.symbol_a,
                &p.symbol_b,
                c,
                period,
            )?;
            n += 1;
        }
    }
    Ok(n)
}

#[allow(dead_code)]
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

pub fn compute_and_store_default_snapshots_backend(backend: &BackendConnection) -> Result<usize> {
    let held = collect_held_symbols_backend(backend);
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
            let a = load_closes_with_fallback_backend(backend, &held_symbol, 120).map(|(_, v)| v);
            let b = load_closes_with_fallback_backend(backend, macro_symbol, 120).map(|(_, v)| v);
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
                    let _ = correlation_snapshots::store_snapshot_backend(
                        backend,
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

/// List correlation break pairs: where |corr_7d − corr_90d| >= threshold.
/// Optionally seed recurring technical alerts for each break pair.
pub fn run_breaks(
    backend: &BackendConnection,
    threshold: f64,
    limit: usize,
    seed_alerts: bool,
    cooldown: i64,
    json: bool,
) -> Result<()> {
    if threshold <= 0.0 || threshold > 1.0 {
        anyhow::bail!("--threshold must be between 0.0 (exclusive) and 1.0 (inclusive)");
    }

    // Compute all pairs at 90d window (we need both 7d and 90d values)
    let pairs = compute_breaks_backend(backend, threshold, limit)?;

    if seed_alerts && !pairs.is_empty() {
        let existing = db_alerts::list_alerts_backend(backend)?;
        let mut seeded = 0usize;
        for bp in &pairs {
            let symbol_key = format!("{}:{}", bp.symbol_a, bp.symbol_b);
            let already_exists = existing.iter().any(|a| {
                a.kind == AlertKind::Technical
                    && a.condition.as_deref() == Some("correlation_break")
                    && (a.symbol == symbol_key
                        || a.symbol == format!("{}:{}", bp.symbol_b, bp.symbol_a))
            });
            if already_exists {
                continue;
            }
            let threshold_str = format!("{:.2}", threshold);
            let rule_text = format!(
                "Correlation break {}-{} (delta {:.2}, threshold {})",
                bp.symbol_a, bp.symbol_b, bp.break_delta, threshold_str
            );
            db_alerts::add_alert_backend(
                backend,
                db_alerts::NewAlert {
                    kind: "technical",
                    symbol: &symbol_key,
                    direction: "above",
                    condition: Some("correlation_break"),
                    threshold: &threshold_str,
                    rule_text: &rule_text,
                    recurring: true,
                    cooldown_minutes: cooldown,
                },
            )?;
            seeded += 1;
        }
        if !json {
            println!("Seeded {} correlation break alerts.", seeded);
        }
    }

    if json {
        let output = serde_json::json!({
            "threshold": threshold,
            "breaks": pairs.iter().map(|bp| serde_json::json!({
                "symbol_a": bp.symbol_a,
                "symbol_b": bp.symbol_b,
                "corr_7d": bp.corr_7d,
                "corr_90d": bp.corr_90d,
                "break_delta": bp.break_delta,
            })).collect::<Vec<_>>(),
            "count": pairs.len(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if pairs.is_empty() {
        println!(
            "No correlation breaks detected at threshold {:.2}. All pairs are stable.",
            threshold
        );
    } else {
        println!("Correlation Breaks (|7d − 90d| ≥ {:.2})", threshold);
        println!();
        println!("{:<22} {:>8} {:>8} {:>10}", "Pair", "7d", "90d", "Break Δ");
        println!("{}", "─".repeat(52));
        for bp in &pairs {
            let pair = format!("{}-{}", bp.symbol_a, bp.symbol_b);
            println!(
                "{:<22} {:>8} {:>8} {:>10}",
                truncate(&pair, 22),
                fmt_opt(bp.corr_7d),
                fmt_opt(bp.corr_90d),
                format!("{:+.2}", bp.break_delta),
            );
        }
    }

    Ok(())
}

/// A pair whose short-term vs long-term correlation has diverged beyond threshold.
#[derive(Debug, Clone)]
pub struct CorrelationBreak {
    pub symbol_a: String,
    pub symbol_b: String,
    pub corr_7d: Option<f64>,
    pub corr_90d: Option<f64>,
    pub break_delta: f64,
}

/// Compute correlation break pairs from fresh rolling correlations.
pub fn compute_breaks_backend(
    backend: &BackendConnection,
    threshold: f64,
    limit: usize,
) -> Result<Vec<CorrelationBreak>> {
    // Get all pairs — use 90 as primary window, high limit to see all pairs
    let all_pairs = compute_pairs_backend(backend, 90, 500)?;
    let mut breaks: Vec<CorrelationBreak> = all_pairs
        .into_iter()
        .filter_map(|p| match (p.corr_7d, p.corr_90d) {
            (Some(c7), Some(c90)) => {
                let delta = c7 - c90;
                if delta.abs() >= threshold {
                    Some(CorrelationBreak {
                        symbol_a: p.symbol_a,
                        symbol_b: p.symbol_b,
                        corr_7d: Some(c7),
                        corr_90d: Some(c90),
                        break_delta: delta,
                    })
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    // Sort by absolute delta descending (biggest breaks first)
    breaks.sort_by(|a, b| {
        b.break_delta
            .abs()
            .partial_cmp(&a.break_delta.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    breaks.truncate(limit);
    Ok(breaks)
}

fn print_pairs(
    held: HashSet<String>,
    pairs: Vec<PairCorrelation>,
    window: usize,
    limit: usize,
    json: bool,
) -> Result<()> {
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
        println!(
            "No correlation pairs available yet. Need ~91 daily closes per symbol for 90d windows; run `pftui refresh` to build/backfill history."
        );
        return Ok(());
    }

    println!(
        "Rolling Correlations (sorted by {}d absolute correlation)",
        window
    );
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

fn collect_held_symbols_backend(backend: &BackendConnection) -> HashSet<String> {
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

fn load_closes_with_fallback_backend(
    backend: &BackendConnection,
    symbol: &str,
    limit: u32,
) -> Option<(String, Vec<f64>)> {
    if let Some(closes) = load_closes_backend(backend, symbol, limit) {
        return Some((symbol.to_string(), closes));
    }

    if symbol.ends_with("-USD") {
        let stripped = symbol.trim_end_matches("-USD");
        if let Some(closes) = load_closes_backend(backend, stripped, limit) {
            return Some((stripped.to_string(), closes));
        }
    } else {
        let usd = format!("{}-USD", symbol);
        if let Some(closes) = load_closes_backend(backend, &usd, limit) {
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

fn load_closes_backend(backend: &BackendConnection, symbol: &str, limit: u32) -> Option<Vec<f64>> {
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

/// Run `analytics correlations latest --with-impact --json`
/// Enriches each correlation snapshot (especially breaks) with portfolio impact data.
pub fn run_latest_with_impact(
    backend: &BackendConnection,
    period: Option<&str>,
    limit: usize,
) -> Result<()> {
    use crate::db::allocations::list_allocations_backend;
    use crate::db::price_cache::get_all_cached_prices_backend;
    use crate::db::transactions::list_transactions_backend;
    use crate::models::position::compute_positions;
    use rust_decimal::Decimal;

    // 1. Get stored correlation snapshots
    let mut rows = correlation_snapshots::list_current_backend(backend, period)?;
    rows.sort_by(|a, b| {
        b.correlation
            .abs()
            .partial_cmp(&a.correlation.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows.truncate(limit.max(1));

    // 2. Compute portfolio positions
    let txns = list_transactions_backend(backend)?;
    let all_prices = get_all_cached_prices_backend(backend)?;
    let mut price_map: HashMap<String, Decimal> = HashMap::new();
    for q in &all_prices {
        price_map.insert(q.symbol.clone(), q.price);
    }
    let positions = compute_positions(&txns, &price_map, &HashMap::new());

    // Build a set of held symbols
    let held: HashSet<String> = positions.iter().map(|p| p.symbol.clone()).collect();

    // Also get allocation-based symbols
    let alloc_syms: HashSet<String> = list_allocations_backend(backend)
        .unwrap_or_default()
        .iter()
        .map(|a| a.symbol.clone())
        .collect();

    let all_held: HashSet<String> = held.union(&alloc_syms).cloned().collect();

    // 3. Compute fresh breaks for enrichment
    let breaks = compute_breaks_backend(backend, 0.20, 100)?;
    let break_map: HashMap<(String, String), f64> = breaks
        .iter()
        .map(|b| {
            (
                (b.symbol_a.clone(), b.symbol_b.clone()),
                b.break_delta,
            )
        })
        .collect();

    // 4. Enrich each snapshot row with impact data
    let enriched: Vec<serde_json::Value> = rows
        .iter()
        .map(|snap| {
            let break_delta = break_map
                .get(&(snap.symbol_a.clone(), snap.symbol_b.clone()))
                .or_else(|| break_map.get(&(snap.symbol_b.clone(), snap.symbol_a.clone())));

            let is_break = break_delta.is_some();

            // Find which held positions are affected by this pair
            let a_held = all_held.contains(&snap.symbol_a);
            let b_held = all_held.contains(&snap.symbol_b);
            let affected_positions: Vec<serde_json::Value> = positions
                .iter()
                .filter(|p| p.symbol == snap.symbol_a || p.symbol == snap.symbol_b)
                .map(|p| {
                    // For a break, estimate direction of impact:
                    // If correlation was positive and broke lower → the pair is decoupling
                    // If correlation was negative and broke higher → they're converging
                    let impact_direction = if let Some(&delta) = break_delta {
                        if p.symbol == snap.symbol_a {
                            // Symbol A: if delta > 0, short-term corr rose above long-term → positive for A relative to B
                            if delta > 0.0 {
                                "positive"
                            } else {
                                "negative"
                            }
                        } else {
                            // Symbol B: reverse perspective
                            if snap.correlation > 0.0 {
                                if delta > 0.0 { "positive" } else { "negative" }
                            } else if delta > 0.0 {
                                "negative"
                            } else {
                                "positive"
                            }
                        }
                    } else {
                        "neutral"
                    };

                    // Magnitude estimate based on correlation strength and position size
                    let magnitude = match p.allocation_pct {
                        Some(pct) => {
                            let corr_strength = snap.correlation.abs();
                            let alloc = pct.to_string().parse::<f64>().unwrap_or(0.0);
                            let magnitude_score = corr_strength * (alloc / 100.0);
                            if magnitude_score > 0.15 {
                                "high"
                            } else if magnitude_score > 0.05 {
                                "medium"
                            } else {
                                "low"
                            }
                        }
                        None => "unknown",
                    };

                    serde_json::json!({
                        "symbol": p.symbol,
                        "name": p.name,
                        "category": format!("{:?}", p.category).to_lowercase(),
                        "allocation_pct": p.allocation_pct.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
                        "current_value": p.current_value.map(|d| d.to_string().parse::<f64>().unwrap_or(0.0)),
                        "impact_direction": impact_direction,
                        "magnitude": magnitude,
                    })
                })
                .collect();

            let mut snap_json = serde_json::json!({
                "symbol_a": snap.symbol_a,
                "symbol_b": snap.symbol_b,
                "correlation": snap.correlation,
                "period": snap.period,
                "recorded_at": snap.recorded_at,
                "is_break": is_break,
            });

            if let Some(&delta) = break_delta {
                snap_json["break_delta"] = serde_json::json!(delta);
            }

            if !affected_positions.is_empty() {
                snap_json["portfolio_impact"] = serde_json::json!({
                    "affected_positions": affected_positions,
                    "a_is_held": a_held,
                    "b_is_held": b_held,
                });
            }

            snap_json
        })
        .collect();

    let output = serde_json::json!({
        "period": period,
        "with_impact": true,
        "snapshots": enriched,
        "count": enriched.len(),
        "held_symbols": all_held.into_iter().collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
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
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::price_history;
    use crate::models::price::HistoryRecord;
    use rust_decimal::Decimal;

    /// Insert synthetic price history for a symbol: `count` days of
    /// linearly ascending closes starting from `base_price`.
    fn seed_history(conn: &rusqlite::Connection, symbol: &str, base_price: f64, count: usize) {
        let base_date = chrono::NaiveDate::from_ymd_opt(2025, 10, 1).unwrap();
        let records: Vec<HistoryRecord> = (0..count)
            .map(|day| {
                let price = base_price + day as f64;
                HistoryRecord {
                    date: (base_date + chrono::Duration::days(day as i64))
                        .format("%Y-%m-%d")
                        .to_string(),
                    close: Decimal::from_str_exact(&format!("{price:.2}")).unwrap_or_default(),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect();
        price_history::upsert_history(conn, symbol, "test", &records).unwrap();
    }

    /// Seed a held position so the symbol shows up in correlation pair candidates.
    fn seed_position(conn: &rusqlite::Connection, symbol: &str) {
        crate::db::transactions::insert_transaction(
            conn,
            &crate::models::transaction::NewTransaction {
                symbol: symbol.to_string(),
                category: crate::models::asset::AssetCategory::Commodity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: Decimal::from(1),
                price_per: Decimal::from(100),
                currency: "USD".to_string(),
                date: "2025-10-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn test_compute_breaks_empty_db() {
        let backend = BackendConnection::Sqlite {
            conn: open_in_memory(),
        };
        let breaks = compute_breaks_backend(&backend, 0.30, 20).unwrap();
        assert!(breaks.is_empty());
    }

    #[test]
    fn test_compute_breaks_no_divergence_returns_empty() {
        let conn = open_in_memory();
        // Two symbols with identical price series → correlation ≈ 1.0 at all windows → no break
        seed_position(&conn, "AAA");
        seed_history(&conn, "AAA", 100.0, 120);
        seed_history(&conn, "SPY", 200.0, 120);
        let backend = BackendConnection::Sqlite { conn };
        let breaks = compute_breaks_backend(&backend, 0.30, 20).unwrap();
        assert!(breaks.is_empty(), "Parallel series should have no break");
    }

    #[test]
    fn test_compute_breaks_detects_divergence() {
        let conn = open_in_memory();
        seed_position(&conn, "DIV");
        // DIV: ascending for 90 days then descending for 30 days (creates short/long divergence)
        let base_date = chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
        let records: Vec<HistoryRecord> = (0..120)
            .map(|day| {
                let price = if day < 90 {
                    100.0 + day as f64
                } else {
                    190.0 - (day - 90) as f64 * 5.0
                };
                HistoryRecord {
                    date: (base_date + chrono::Duration::days(day as i64))
                        .format("%Y-%m-%d")
                        .to_string(),
                    close: Decimal::from_str_exact(&format!("{price:.2}")).unwrap_or_default(),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect();
        price_history::upsert_history(&conn, "DIV", "test", &records).unwrap();
        // SPY: steady ascending
        let spy: Vec<HistoryRecord> = (0..120)
            .map(|day| HistoryRecord {
                date: (base_date + chrono::Duration::days(day as i64))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from_str_exact(&format!("{:.2}", 200.0 + day as f64))
                    .unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        price_history::upsert_history(&conn, "SPY", "test", &spy).unwrap();
        let backend = BackendConnection::Sqlite { conn };
        // Low threshold to catch the divergence
        let breaks = compute_breaks_backend(&backend, 0.10, 20).unwrap();
        // The DIV-SPY pair should show a break since recent behavior flipped
        assert!(
            !breaks.is_empty(),
            "Divergent series should produce a break"
        );
        assert!(breaks[0].break_delta.abs() >= 0.10);
    }

    #[test]
    fn test_run_breaks_seed_alerts_creates_alerts() {
        let conn = open_in_memory();
        seed_position(&conn, "SEED");
        // Create a pair with known divergence
        let base_date = chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
        let records: Vec<HistoryRecord> = (0..120)
            .map(|day| {
                let price = if day < 90 {
                    100.0 + day as f64
                } else {
                    190.0 - (day - 90) as f64 * 5.0
                };
                HistoryRecord {
                    date: (base_date + chrono::Duration::days(day as i64))
                        .format("%Y-%m-%d")
                        .to_string(),
                    close: Decimal::from_str_exact(&format!("{price:.2}")).unwrap_or_default(),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect();
        price_history::upsert_history(&conn, "SEED", "test", &records).unwrap();
        let spy: Vec<HistoryRecord> = (0..120)
            .map(|day| HistoryRecord {
                date: (base_date + chrono::Duration::days(day as i64))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from_str_exact(&format!("{:.2}", 200.0 + day as f64))
                    .unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        price_history::upsert_history(&conn, "SPY", "test", &spy).unwrap();

        let backend = BackendConnection::Sqlite { conn };
        // Run breaks with seed-alerts
        run_breaks(&backend, 0.10, 20, true, 240, false).unwrap();

        let alerts = db_alerts::list_alerts_backend(&backend).unwrap();
        let corr_alerts: Vec<_> = alerts
            .iter()
            .filter(|a| a.condition.as_deref() == Some("correlation_break"))
            .collect();
        assert!(
            !corr_alerts.is_empty(),
            "Should have seeded correlation break alerts"
        );
        assert!(corr_alerts[0].recurring);
        assert_eq!(corr_alerts[0].cooldown_minutes, 240);
    }

    #[test]
    fn test_run_breaks_seed_alerts_no_duplicates() {
        let conn = open_in_memory();
        seed_position(&conn, "DUP");
        let base_date = chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
        let records: Vec<HistoryRecord> = (0..120)
            .map(|day| {
                let price = if day < 90 {
                    100.0 + day as f64
                } else {
                    190.0 - (day - 90) as f64 * 5.0
                };
                HistoryRecord {
                    date: (base_date + chrono::Duration::days(day as i64))
                        .format("%Y-%m-%d")
                        .to_string(),
                    close: Decimal::from_str_exact(&format!("{price:.2}")).unwrap_or_default(),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect();
        price_history::upsert_history(&conn, "DUP", "test", &records).unwrap();
        let spy: Vec<HistoryRecord> = (0..120)
            .map(|day| HistoryRecord {
                date: (base_date + chrono::Duration::days(day as i64))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from_str_exact(&format!("{:.2}", 200.0 + day as f64))
                    .unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        price_history::upsert_history(&conn, "SPY", "test", &spy).unwrap();

        let backend = BackendConnection::Sqlite { conn };
        // Seed twice — second run should not duplicate
        run_breaks(&backend, 0.10, 20, true, 240, false).unwrap();
        run_breaks(&backend, 0.10, 20, true, 240, false).unwrap();

        let alerts = db_alerts::list_alerts_backend(&backend).unwrap();
        let corr_alerts: Vec<_> = alerts
            .iter()
            .filter(|a| a.condition.as_deref() == Some("correlation_break"))
            .collect();
        // Each pair should appear at most once
        let mut seen = HashSet::new();
        for a in &corr_alerts {
            let key = a.symbol.clone();
            assert!(
                seen.insert(key.clone()),
                "Duplicate correlation break alert for {}",
                key
            );
        }
    }

    #[test]
    fn test_breaks_sorted_by_absolute_delta() {
        let conn = open_in_memory();
        seed_position(&conn, "BIG");
        seed_position(&conn, "SML");
        let base_date = chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();

        // BIG: strong reversal at day 90
        let big: Vec<HistoryRecord> = (0..120)
            .map(|day| {
                let price = if day < 90 {
                    100.0 + day as f64
                } else {
                    190.0 - (day - 90) as f64 * 10.0
                };
                HistoryRecord {
                    date: (base_date + chrono::Duration::days(day as i64))
                        .format("%Y-%m-%d")
                        .to_string(),
                    close: Decimal::from_str_exact(&format!("{price:.2}")).unwrap_or_default(),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect();
        price_history::upsert_history(&conn, "BIG", "test", &big).unwrap();

        // SML: mild reversal at day 90
        let sml: Vec<HistoryRecord> = (0..120)
            .map(|day| {
                let price = if day < 90 {
                    100.0 + day as f64
                } else {
                    190.0 - (day - 90) as f64 * 2.0
                };
                HistoryRecord {
                    date: (base_date + chrono::Duration::days(day as i64))
                        .format("%Y-%m-%d")
                        .to_string(),
                    close: Decimal::from_str_exact(&format!("{price:.2}")).unwrap_or_default(),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect();
        price_history::upsert_history(&conn, "SML", "test", &sml).unwrap();

        // SPY: steadily ascending
        let spy: Vec<HistoryRecord> = (0..120)
            .map(|day| HistoryRecord {
                date: (base_date + chrono::Duration::days(day as i64))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from_str_exact(&format!("{:.2}", 200.0 + day as f64))
                    .unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        price_history::upsert_history(&conn, "SPY", "test", &spy).unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let breaks = compute_breaks_backend(&backend, 0.01, 50).unwrap();
        // Results should be sorted by absolute delta descending
        for window in breaks.windows(2) {
            assert!(
                window[0].break_delta.abs() >= window[1].break_delta.abs(),
                "Breaks should be sorted by |delta| descending"
            );
        }
    }
}
