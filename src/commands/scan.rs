use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use crate::analytics::technicals::{
    load_or_compute_snapshots, load_or_compute_snapshots_backend, DEFAULT_TIMEFRAME,
};
use crate::config::{currency_symbol, Config, PortfolioMode};
use crate::db::allocations::{list_allocations, list_allocations_backend};
use crate::db::backend::BackendConnection;
use crate::db::fx_cache::get_all_fx_rates;
use crate::db::news_cache::{get_latest_news_backend, NewsEntry};
use crate::db::price_cache::{get_all_cached_prices, get_all_cached_prices_backend};
use crate::db::scan_queries::{
    get_scan_query_backend, list_scan_queries_backend, upsert_scan_query_backend,
};
use crate::db::technical_snapshots::TechnicalSnapshotRecord;
use crate::db::transactions::{list_transactions, list_transactions_backend};
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Ne,
    Contains,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Clause {
    field: String,
    op: FilterOp,
    value: String,
}

struct ScanRow {
    symbol: String,
    name: String,
    category: String,
    quantity: Option<Decimal>,
    current_price: Option<Decimal>,
    current_value: Option<Decimal>,
    gain_pct: Option<Decimal>,
    allocation_pct: Option<Decimal>,
    sma50: Option<Decimal>,
    sma200: Option<Decimal>,
    sma50_gap_pct: Option<Decimal>,
    sma200_gap_pct: Option<Decimal>,
    trackline_breach: String,
    trackline_breach_count: Decimal,
}

type TechnicalSnapshot = TechnicalSnapshotRecord;

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    config: &Config,
    filter: Option<&str>,
    save: Option<&str>,
    load: Option<&str>,
    list: bool,
    news_keyword: Option<&str>,
    trackline_breaches: bool,
    json: bool,
) -> Result<()> {
    if list {
        return print_saved_queries(backend, json);
    }

    let mut effective_filter: Option<String> = filter.map(|s| s.to_string());
    if let Some(query_name) = load {
        let Some(saved) = get_scan_query_backend(backend, query_name)? else {
            bail!("Saved scan query '{}' not found", query_name);
        };
        effective_filter = Some(saved.filter_expr);
    }
    if trackline_breaches {
        let breach_clause = "trackline_breach != none";
        effective_filter = Some(match effective_filter {
            Some(existing) => format!("{existing} and {breach_clause}"),
            None => breach_clause.to_string(),
        });
    }
    let Some(filter_expr) = effective_filter else {
        bail!(
            "Missing filter. Use --filter, --load, --trackline-breaches, or --list. Example: pftui analytics scan --filter \"allocation_pct > 10\""
        );
    };

    if let Some(name) = save {
        upsert_scan_query_backend(backend, name, &filter_expr)?;
        if !json {
            println!("Saved scan query '{}' as: {}", name, filter_expr);
            println!();
        }
    }

    let clauses = parse_filter(&filter_expr)?;
    validate_clauses(&clauses)?;

    let rows = load_rows_backend(backend, Some(config.portfolio_mode))?;
    if rows.is_empty() {
        println!("No positions to scan. Add holdings first.");
        return Ok(());
    }

    let matching_news = if let Some(keyword) = news_keyword {
        get_latest_news_backend(backend, 200, None, None, Some(keyword), Some(168))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut matches: Vec<&ScanRow> = Vec::new();
    for row in &rows {
        let clause_match = matches_all_clauses(row, &clauses)?;
        if !clause_match {
            continue;
        }
        let news_match = if news_keyword.is_some() {
            row_matches_news(row, &matching_news)
        } else {
            true
        };
        if news_match {
            matches.push(row);
        }
    }

    if json {
        let output = serde_json::json!({
            "filter": filter_expr,
            "news_keyword": news_keyword,
            "matching_news_count": matching_news.len(),
            "total_scanned": rows.len(),
            "match_count": matches.len(),
            "matches": matches.into_iter().map(to_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    print_table(
        matches,
        rows.len(),
        &filter_expr,
        &config.base_currency,
        news_keyword,
        matching_news.len(),
        trackline_breaches || filter_expr.contains("trackline") || filter_expr.contains("sma"),
    );
    Ok(())
}

pub fn count_matches(conn: &Connection, filter: &str) -> Result<usize> {
    let clauses = parse_filter(filter)?;
    validate_clauses(&clauses)?;
    let rows = load_rows(conn, None)?;
    let mut count = 0usize;
    for row in &rows {
        if matches_all_clauses(row, &clauses)? {
            count += 1;
        }
    }
    Ok(count)
}

pub fn count_matches_backend(backend: &BackendConnection, filter: &str) -> Result<usize> {
    let clauses = parse_filter(filter)?;
    validate_clauses(&clauses)?;
    let rows = load_rows_backend(backend, None)?;
    let mut count = 0usize;
    for row in &rows {
        if matches_all_clauses(row, &clauses)? {
            count += 1;
        }
    }
    Ok(count)
}

fn print_saved_queries(backend: &BackendConnection, json: bool) -> Result<()> {
    let rows = list_scan_queries_backend(backend)?;
    if json {
        let output = serde_json::json!({
            "count": rows.len(),
            "queries": rows.into_iter().map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "filter": r.filter_expr,
                    "updated_at": r.updated_at,
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if rows.is_empty() {
        println!("No saved scan queries.");
        return Ok(());
    }

    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    println!("Saved scans ({}):", rows.len());
    println!("  {:<name_w$}  {:<40}  Updated", "Name", "Filter",);
    println!("  {}", "─".repeat(name_w + 56));
    for row in rows {
        println!(
            "  {:<name_w$}  {:<40}  {}",
            row.name,
            truncate_name(&row.filter_expr, 40),
            row.updated_at
        );
    }
    Ok(())
}

fn load_rows(conn: &Connection, mode_hint: Option<PortfolioMode>) -> Result<Vec<ScanRow>> {
    let prices = get_all_cached_prices(conn)?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();
    let fx_rates = get_all_fx_rates(conn).unwrap_or_default();
    let txs = list_transactions(conn)?;
    let use_mode = match mode_hint {
        Some(m) => m,
        None => {
            if txs.is_empty() {
                PortfolioMode::Percentage
            } else {
                PortfolioMode::Full
            }
        }
    };

    let positions: Vec<Position> = match use_mode {
        PortfolioMode::Full => compute_positions(&txs, &prices, &fx_rates),
        PortfolioMode::Percentage => {
            let allocs = list_allocations(conn)?;
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };

    let symbols: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();
    let technicals = load_technical_snapshots(conn, &symbols);

    Ok(positions
        .into_iter()
        .map(|p| {
            let current_price = p.current_price;
            let technical = technicals.get(&p.symbol);
            let sma50 = technical.and_then(|t| f64_to_decimal(t.sma_50));
            let sma200 = technical.and_then(|t| f64_to_decimal(t.sma_200));
            let sma50_gap_pct = compute_gap_pct(current_price, sma50);
            let sma200_gap_pct = compute_gap_pct(current_price, sma200);
            let trackline_breach = build_trackline_breach(sma50_gap_pct, sma200_gap_pct);
            let trackline_breach_count = if trackline_breach == "none" {
                Decimal::ZERO
            } else {
                Decimal::from(trackline_breach.split(',').count() as i64)
            };
            ScanRow {
                symbol: p.symbol,
                name: p.name,
                category: p.category.to_string(),
                quantity: if p.quantity == dec!(0) {
                    None
                } else {
                    Some(p.quantity)
                },
                current_price,
                current_value: p.current_value,
                gain_pct: p.gain_pct,
                allocation_pct: p.allocation_pct,
                sma50,
                sma200,
                sma50_gap_pct,
                sma200_gap_pct,
                trackline_breach_count,
                trackline_breach,
            }
        })
        .collect())
}

fn load_rows_backend(
    backend: &BackendConnection,
    mode_hint: Option<PortfolioMode>,
) -> Result<Vec<ScanRow>> {
    let prices = get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let txs = list_transactions_backend(backend)?;
    let use_mode = match mode_hint {
        Some(m) => m,
        None => {
            if txs.is_empty() {
                PortfolioMode::Percentage
            } else {
                PortfolioMode::Full
            }
        }
    };

    let positions: Vec<Position> = match use_mode {
        PortfolioMode::Full => compute_positions(&txs, &prices, &fx_rates),
        PortfolioMode::Percentage => {
            let allocs = list_allocations_backend(backend)?;
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };

    let symbols: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();
    let technicals = load_technical_snapshots_backend(backend, &symbols);

    Ok(positions
        .into_iter()
        .map(|p| {
            let current_price = p.current_price;
            let technical = technicals.get(&p.symbol);
            let sma50 = technical.and_then(|t| f64_to_decimal(t.sma_50));
            let sma200 = technical.and_then(|t| f64_to_decimal(t.sma_200));
            let sma50_gap_pct = compute_gap_pct(current_price, sma50);
            let sma200_gap_pct = compute_gap_pct(current_price, sma200);
            let trackline_breach = build_trackline_breach(sma50_gap_pct, sma200_gap_pct);
            let trackline_breach_count = if trackline_breach == "none" {
                Decimal::ZERO
            } else {
                Decimal::from(trackline_breach.split(',').count() as i64)
            };
            ScanRow {
                symbol: p.symbol,
                name: p.name,
                category: p.category.to_string(),
                quantity: if p.quantity == dec!(0) {
                    None
                } else {
                    Some(p.quantity)
                },
                current_price,
                current_value: p.current_value,
                gain_pct: p.gain_pct,
                allocation_pct: p.allocation_pct,
                sma50,
                sma200,
                sma50_gap_pct,
                sma200_gap_pct,
                trackline_breach_count,
                trackline_breach,
            }
        })
        .collect())
}

fn load_technical_snapshots(
    conn: &Connection,
    symbols: &[String],
) -> HashMap<String, TechnicalSnapshot> {
    load_or_compute_snapshots(conn, symbols, DEFAULT_TIMEFRAME)
}

fn load_technical_snapshots_backend(
    backend: &BackendConnection,
    symbols: &[String],
) -> HashMap<String, TechnicalSnapshot> {
    load_or_compute_snapshots_backend(backend, symbols, DEFAULT_TIMEFRAME)
}

fn compute_gap_pct(current_price: Option<Decimal>, reference: Option<Decimal>) -> Option<Decimal> {
    let current = current_price?;
    let reference = reference?;
    if reference.is_zero() {
        return None;
    }
    Some((current - reference) / reference * Decimal::from(100))
}

fn f64_to_decimal(value: Option<f64>) -> Option<Decimal> {
    value.and_then(|v| Decimal::from_str_exact(&format!("{v:.6}")).ok())
}

fn build_trackline_breach(
    sma50_gap_pct: Option<Decimal>,
    sma200_gap_pct: Option<Decimal>,
) -> String {
    let mut breaches = Vec::new();
    if sma50_gap_pct.is_some_and(|gap| gap < Decimal::ZERO) {
        breaches.push("below_sma50");
    }
    if sma200_gap_pct.is_some_and(|gap| gap < Decimal::ZERO) {
        breaches.push("below_sma200");
    }
    if breaches.is_empty() {
        "none".to_string()
    } else {
        breaches.join(",")
    }
}

fn parse_filter(input: &str) -> Result<Vec<Clause>> {
    let tokens = tokenize(input);
    if tokens.is_empty() {
        bail!("Filter is empty. Example: pftui scan --filter \"allocation_pct > 10\"");
    }

    let mut clauses = Vec::new();
    let mut idx = 0usize;
    while idx < tokens.len() {
        if idx + 2 >= tokens.len() {
            bail!(
                "Invalid filter near '{}'. Expected: <field> <op> <value>",
                tokens[idx]
            );
        }
        let field = tokens[idx].to_lowercase();
        let op = parse_op(&tokens[idx + 1])?;
        let value = tokens[idx + 2].clone();
        clauses.push(Clause { field, op, value });
        idx += 3;

        if idx < tokens.len() {
            let connector = tokens[idx].to_lowercase();
            if connector != "and" && connector != "&&" {
                bail!(
                    "Unsupported connector '{}'. Use 'and' or '&&' between clauses.",
                    tokens[idx]
                );
            }
            idx += 1;
        }
    }
    Ok(clauses)
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in input.chars() {
        match quote {
            Some(q) if ch == q => {
                quote = None;
            }
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
            }
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn parse_op(op: &str) -> Result<FilterOp> {
    match op {
        ">" => Ok(FilterOp::Gt),
        ">=" => Ok(FilterOp::Gte),
        "<" => Ok(FilterOp::Lt),
        "<=" => Ok(FilterOp::Lte),
        "=" | "==" => Ok(FilterOp::Eq),
        "!=" => Ok(FilterOp::Ne),
        "contains" | "~" => Ok(FilterOp::Contains),
        _ => bail!("Unsupported operator '{}'", op),
    }
}

fn validate_clauses(clauses: &[Clause]) -> Result<()> {
    for clause in clauses {
        let Some(field_type) = field_type(&clause.field) else {
            bail!(
                "Unknown field '{}'. Supported fields: symbol, name, category, quantity, current_price, current_value, gain_pct, allocation_pct, sma50, sma200, sma50_gap_pct, sma200_gap_pct, trackline_breach, trackline_breach_count",
                clause.field
            );
        };
        match field_type {
            FieldType::Numeric => {
                if clause.op == FilterOp::Contains {
                    bail!("'contains' is only valid for text fields");
                }
                clause
                    .value
                    .parse::<Decimal>()
                    .with_context(|| format!("'{}' is not a valid number", clause.value))?;
            }
            FieldType::Text => {
                if !matches!(clause.op, FilterOp::Eq | FilterOp::Ne | FilterOp::Contains) {
                    bail!("Text fields only support ==, !=, and contains operators");
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum FieldType {
    Numeric,
    Text,
}

fn field_type(field: &str) -> Option<FieldType> {
    match normalize_field(field) {
        "symbol" | "name" | "category" | "trackline_breach" => Some(FieldType::Text),
        "quantity"
        | "current_price"
        | "current_value"
        | "gain_pct"
        | "allocation_pct"
        | "sma50"
        | "sma200"
        | "sma50_gap_pct"
        | "sma200_gap_pct"
        | "trackline_breach_count" => Some(FieldType::Numeric),
        _ => None,
    }
}

fn normalize_field(field: &str) -> &str {
    match field {
        "alloc" | "allocation" => "allocation_pct",
        "gain" => "gain_pct",
        "price" => "current_price",
        "value" => "current_value",
        "qty" => "quantity",
        "sma_50" => "sma50",
        "sma_200" => "sma200",
        "sma50_gap" => "sma50_gap_pct",
        "sma200_gap" => "sma200_gap_pct",
        "breach" => "trackline_breach",
        other => other,
    }
}

fn matches_all_clauses(row: &ScanRow, clauses: &[Clause]) -> Result<bool> {
    for clause in clauses {
        if !matches_clause(row, clause)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_clause(row: &ScanRow, clause: &Clause) -> Result<bool> {
    match field_type(&clause.field).context("invalid field")? {
        FieldType::Text => {
            let lhs =
                get_text(row, normalize_field(&clause.field)).context("missing text field")?;
            let lhs = lhs.to_lowercase();
            let rhs = clause.value.to_lowercase();
            Ok(match clause.op {
                FilterOp::Eq => lhs == rhs,
                FilterOp::Ne => lhs != rhs,
                FilterOp::Contains => lhs.contains(&rhs),
                _ => false,
            })
        }
        FieldType::Numeric => {
            let Some(lhs) = get_numeric(row, normalize_field(&clause.field)) else {
                return Ok(false);
            };
            let rhs = clause
                .value
                .parse::<Decimal>()
                .with_context(|| format!("invalid numeric value '{}'", clause.value))?;
            Ok(match clause.op {
                FilterOp::Gt => lhs > rhs,
                FilterOp::Gte => lhs >= rhs,
                FilterOp::Lt => lhs < rhs,
                FilterOp::Lte => lhs <= rhs,
                FilterOp::Eq => lhs == rhs,
                FilterOp::Ne => lhs != rhs,
                FilterOp::Contains => false,
            })
        }
    }
}

fn get_text<'a>(row: &'a ScanRow, field: &str) -> Option<&'a str> {
    match field {
        "symbol" => Some(row.symbol.as_str()),
        "name" => Some(row.name.as_str()),
        "category" => Some(row.category.as_str()),
        "trackline_breach" => Some(row.trackline_breach.as_str()),
        _ => None,
    }
}

fn get_numeric(row: &ScanRow, field: &str) -> Option<Decimal> {
    match field {
        "quantity" => row.quantity,
        "current_price" => row.current_price,
        "current_value" => row.current_value,
        "gain_pct" => row.gain_pct,
        "allocation_pct" => row.allocation_pct,
        "sma50" => row.sma50,
        "sma200" => row.sma200,
        "sma50_gap_pct" => row.sma50_gap_pct,
        "sma200_gap_pct" => row.sma200_gap_pct,
        "trackline_breach_count" => Some(row.trackline_breach_count),
        _ => None,
    }
}

fn print_table(
    rows: Vec<&ScanRow>,
    total_scanned: usize,
    filter: &str,
    base_currency: &str,
    news_keyword: Option<&str>,
    matching_news_count: usize,
    show_trackline: bool,
) {
    println!(
        "Scan results: {}/{} matched (`{}`)\n",
        rows.len(),
        total_scanned,
        filter
    );
    if let Some(keyword) = news_keyword {
        println!(
            "News keyword filter: '{}' ({} recent matching articles)\n",
            keyword, matching_news_count
        );
    }

    if rows.is_empty() {
        println!("No matches.");
        return;
    }

    let csym = currency_symbol(base_currency);
    let sym_w = rows
        .iter()
        .map(|r| r.symbol.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4)
        .clamp(4, 30);
    let cat_w = rows
        .iter()
        .map(|r| r.category.len())
        .max()
        .unwrap_or(8)
        .max(8);

    if show_trackline {
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>7}  {:>8}  {:>11}  {:>11}  {:<22}",
            "Symbol", "Name", "Category", "Alloc%", "Gain%", "Value", "Price", "Trackline",
        );
        println!("  {}", "─".repeat(sym_w + name_w + cat_w + 74));
    } else {
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>7}  {:>8}  {:>11}  {:>11}",
            "Symbol", "Name", "Category", "Alloc%", "Gain%", "Value", "Price",
        );
        println!("  {}", "─".repeat(sym_w + name_w + cat_w + 50));
    }

    for row in rows {
        let alloc = row
            .allocation_pct
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "-".to_string());
        let gain = row
            .gain_pct
            .map(|v| format!("{:+.2}", v))
            .unwrap_or_else(|| "-".to_string());
        let value = row
            .current_value
            .map(|v| format!("{}{:.2}", csym, v))
            .unwrap_or_else(|| "-".to_string());
        let price = row
            .current_price
            .map(|v| format!("{}{:.2}", csym, v))
            .unwrap_or_else(|| "-".to_string());
        if show_trackline {
            println!(
                "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>7}  {:>8}  {:>11}  {:>11}  {:<22}",
                row.symbol,
                truncate_name(&row.name, name_w),
                row.category,
                alloc,
                gain,
                value,
                price,
                row.trackline_breach,
            );
        } else {
            println!(
                "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>7}  {:>8}  {:>11}  {:>11}",
                row.symbol,
                truncate_name(&row.name, name_w),
                row.category,
                alloc,
                gain,
                value,
                price,
            );
        }
    }
}

fn row_matches_news(row: &ScanRow, entries: &[NewsEntry]) -> bool {
    let symbol = row.symbol.to_ascii_uppercase();
    for n in entries {
        if n.symbol_tag
            .as_ref()
            .map(|s| s.eq_ignore_ascii_case(&symbol))
            .unwrap_or(false)
        {
            return true;
        }
        let text = format!(
            "{} {} {}",
            n.title,
            n.description,
            n.extra_snippets.join(" ")
        )
        .to_ascii_uppercase();
        if text.contains(&symbol) {
            return true;
        }
    }
    false
}

fn truncate_name(name: &str, width: usize) -> String {
    if name.chars().count() <= width {
        return name.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let truncated: String = name.chars().take(width - 1).collect();
    format!("{}…", truncated)
}

fn to_json(row: &ScanRow) -> serde_json::Value {
    serde_json::json!({
        "symbol": row.symbol,
        "name": row.name,
        "category": row.category,
        "quantity": row.quantity.and_then(|v| v.to_string().parse::<f64>().ok()),
        "current_price": row.current_price.and_then(|v| v.to_string().parse::<f64>().ok()),
        "current_value": row.current_value.and_then(|v| v.to_string().parse::<f64>().ok()),
        "gain_pct": row.gain_pct.and_then(|v| v.to_string().parse::<f64>().ok()),
        "allocation_pct": row.allocation_pct.and_then(|v| v.to_string().parse::<f64>().ok()),
        "sma50": row.sma50.and_then(|v| v.to_string().parse::<f64>().ok()),
        "sma200": row.sma200.and_then(|v| v.to_string().parse::<f64>().ok()),
        "sma50_gap_pct": row.sma50_gap_pct.and_then(|v| v.to_string().parse::<f64>().ok()),
        "sma200_gap_pct": row.sma200_gap_pct.and_then(|v| v.to_string().parse::<f64>().ok()),
        "trackline_breach": row.trackline_breach,
        "trackline_breach_count": row.trackline_breach_count.to_string().parse::<f64>().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::price_cache;
    use crate::db::price_history;
    use crate::models::price::{HistoryRecord, PriceQuote};

    fn sample_row() -> ScanRow {
        ScanRow {
            symbol: "AAPL".to_string(),
            name: "Apple Inc".to_string(),
            category: "equity".to_string(),
            quantity: Some(dec!(10)),
            current_price: Some(dec!(190)),
            current_value: Some(dec!(1900)),
            gain_pct: Some(dec!(15)),
            allocation_pct: Some(dec!(12.5)),
            sma50: Some(dec!(195)),
            sma200: Some(dec!(180)),
            sma50_gap_pct: Some(dec!(-2.56)),
            sma200_gap_pct: Some(dec!(5.56)),
            trackline_breach: "below_sma50".to_string(),
            trackline_breach_count: dec!(1),
        }
    }

    #[test]
    fn parses_single_clause() {
        let clauses = parse_filter("allocation_pct > 10").unwrap();
        assert_eq!(clauses.len(), 1);
        assert_eq!(clauses[0].field, "allocation_pct");
        assert_eq!(clauses[0].op, FilterOp::Gt);
        assert_eq!(clauses[0].value, "10");
    }

    #[test]
    fn parses_and_clauses() {
        let clauses = parse_filter("allocation_pct >= 10 and category == equity").unwrap();
        assert_eq!(clauses.len(), 2);
    }

    #[test]
    fn matches_numeric_and_text() {
        let row = sample_row();
        let clauses = parse_filter("allocation > 10 and category == equity").unwrap();
        assert!(matches_all_clauses(&row, &clauses).unwrap());
    }

    #[test]
    fn supports_contains_operator() {
        let row = sample_row();
        let clauses = parse_filter("name contains \"Apple\"").unwrap();
        assert!(matches_all_clauses(&row, &clauses).unwrap());
    }

    #[test]
    fn supports_trackline_breach_text_filter() {
        let row = sample_row();
        let clauses = parse_filter("trackline_breach contains below_sma50").unwrap();
        assert!(matches_all_clauses(&row, &clauses).unwrap());
    }

    #[test]
    fn supports_trackline_gap_numeric_filter() {
        let row = sample_row();
        let clauses = parse_filter("sma50_gap_pct < 0").unwrap();
        assert!(matches_all_clauses(&row, &clauses).unwrap());
    }

    #[test]
    fn computes_trackline_breach_from_price_history() {
        let conn = open_in_memory();
        crate::db::transactions::insert_transaction(
            &conn,
            &crate::models::transaction::NewTransaction {
                symbol: "AAPL".to_string(),
                category: crate::models::asset::AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: Decimal::from(1),
                price_per: Decimal::from(100),
                currency: "USD".to_string(),
                date: "2026-03-09".to_string(),
                notes: None,
            },
        )
        .unwrap();
        price_cache::upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: Decimal::from(95),
                currency: "USD".to_string(),
                fetched_at: "2026-03-10T00:00:00Z".to_string(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
            },
        )
        .unwrap();
        let base_date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let history: Vec<HistoryRecord> = (0..60)
            .map(|day| HistoryRecord {
                date: (base_date + chrono::Duration::days(day))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        price_history::upsert_history(&conn, "AAPL", "test", &history).unwrap();

        let rows = load_rows(&conn, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trackline_breach, "below_sma50");
    }
}
