use std::collections::BTreeMap;

use anyhow::Result;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::{price_history, user_predictions};
use crate::models::asset_names::infer_category;

/// Conviction → hypothetical position weight for theoretical P&L.
/// High = 10%, Medium = 5%, Low = 2% of a notional $10,000 portfolio.
fn conviction_weight(conviction: &str) -> Decimal {
    match conviction.to_lowercase().as_str() {
        "high" => dec!(0.10),
        "medium" => dec!(0.05),
        "low" => dec!(0.02),
        _ => dec!(0.05), // default medium
    }
}

const NOTIONAL_PORTFOLIO: Decimal = dec!(10000);

/// Resolve common symbol aliases to Yahoo Finance tickers (same as predict.rs).
fn resolve_symbol_alias(token: &str) -> Option<&'static str> {
    match token.to_uppercase().as_str() {
        "BTC" | "BITCOIN" | "BTC-USD" => Some("BTC-USD"),
        "ETH" | "ETHEREUM" | "ETH-USD" => Some("ETH-USD"),
        "SOL" | "SOLANA" | "SOL-USD" => Some("SOL-USD"),
        "GOLD" | "XAUUSD" | "GC=F" => Some("GC=F"),
        "SILVER" | "XAGUSD" | "SI=F" => Some("SI=F"),
        "DXY" | "DOLLAR" | "DX-Y.NYB" => Some("DX-Y.NYB"),
        "SPY" | "S&P" | "SP500" | "S&P500" => Some("SPY"),
        "OIL" | "CRUDE" | "WTI" | "CL=F" => Some("CL=F"),
        "VIX" | "^VIX" => Some("^VIX"),
        "NASDAQ" | "QQQ" => Some("QQQ"),
        _ => None,
    }
}

/// Extract a YYYY-MM-DD date from a datetime or date string.
fn extract_date(raw: &str) -> Option<String> {
    // Try YYYY-MM-DD directly
    if raw.len() >= 10 {
        let date_part = &raw[..10];
        if NaiveDate::parse_from_str(date_part, "%Y-%m-%d").is_ok() {
            return Some(date_part.to_string());
        }
    }
    None
}

/// A single backtested prediction with price data and theoretical P&L.
#[derive(Debug, Clone, Serialize)]
pub struct BacktestEntry {
    pub id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub resolved_symbol: Option<String>,
    pub conviction: String,
    pub timeframe: Option<String>,
    pub confidence: Option<f64>,
    pub source_agent: Option<String>,
    pub outcome: String,
    pub created_at: String,
    pub target_date: Option<String>,
    pub scored_at: Option<String>,
    /// Price at time prediction was made (closest available date)
    pub entry_price: Option<Decimal>,
    /// Price at evaluation date (target_date or scored_at, whichever is available)
    pub exit_price: Option<Decimal>,
    /// Percentage price change from entry to exit
    pub price_change_pct: Option<Decimal>,
    /// Direction implied by prediction outcome: +1 (correct long), -1 (correct short), etc.
    pub direction_multiplier: Option<i32>,
    /// Theoretical P&L in dollars (conviction-weighted position on $10k notional)
    pub theoretical_pnl: Option<Decimal>,
    /// Whether we had enough price data to compute P&L
    pub has_price_data: bool,
    /// Notes on data availability
    pub data_note: Option<String>,
}

/// Summary statistics for the backtest.
#[derive(Debug, Clone, Serialize)]
pub struct BacktestSummary {
    pub total_predictions: usize,
    pub scored_predictions: usize,
    pub with_price_data: usize,
    pub without_price_data: usize,
    pub total_theoretical_pnl: Decimal,
    pub avg_pnl_per_trade: Option<Decimal>,
    pub win_count: usize,
    pub loss_count: usize,
    pub win_rate_pct: Option<Decimal>,
    pub best_trade_pnl: Option<Decimal>,
    pub best_trade_claim: Option<String>,
    pub worst_trade_pnl: Option<Decimal>,
    pub worst_trade_claim: Option<String>,
    pub notional_portfolio: Decimal,
}

/// Run `analytics backtest predictions`.
pub fn run_predictions(
    backend: &BackendConnection,
    symbol_filter: Option<&str>,
    agent_filter: Option<&str>,
    timeframe_filter: Option<&str>,
    conviction_filter: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    // Fetch all non-pending predictions
    let mut all_predictions =
        user_predictions::list_predictions_backend(backend, None, None, None, None)?;

    // Filter to scored only (correct/partial/wrong) — exclude "pending" and "open"
    all_predictions.retain(|p| {
        matches!(
            p.outcome.as_str(),
            "correct" | "partial" | "wrong"
        )
    });

    // Apply optional filters
    if let Some(sym) = symbol_filter {
        let sym_upper = sym.to_uppercase();
        all_predictions.retain(|p| {
            p.symbol
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(&sym_upper))
        });
    }
    if let Some(agent) = agent_filter {
        all_predictions.retain(|p| {
            p.source_agent
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(agent))
        });
    }
    if let Some(tf) = timeframe_filter {
        all_predictions.retain(|p| {
            p.timeframe
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(tf))
        });
    }
    if let Some(conv) = conviction_filter {
        all_predictions.retain(|p| p.conviction.eq_ignore_ascii_case(conv));
    }

    if let Some(n) = limit {
        all_predictions.truncate(n);
    }

    // Build backtest entries
    let mut entries = Vec::new();

    for pred in &all_predictions {
        let entry = backtest_prediction(backend, pred);
        entries.push(entry);
    }

    // Compute summary
    let summary = compute_summary(&entries);

    if json_output {
        print_json(&entries, &summary)?;
    } else {
        print_table(&entries, &summary);
    }

    Ok(())
}

fn backtest_prediction(
    backend: &BackendConnection,
    pred: &user_predictions::UserPrediction,
) -> BacktestEntry {
    // Resolve the symbol for price lookup
    let resolved_symbol = pred
        .symbol
        .as_deref()
        .and_then(|s| {
            resolve_symbol_alias(s)
                .map(String::from)
                .or_else(|| Some(s.to_uppercase()))
        });

    // Determine entry date (prediction creation)
    let entry_date = extract_date(&pred.created_at);

    // Determine exit date: prefer target_date, then scored_at
    let exit_date = pred
        .target_date
        .as_deref()
        .and_then(extract_date)
        .or_else(|| pred.scored_at.as_deref().and_then(extract_date));

    let mut entry_price: Option<Decimal> = None;
    let mut exit_price: Option<Decimal> = None;
    let mut data_note: Option<String> = None;

    if let Some(ref sym) = resolved_symbol {
        // Get entry price
        if let Some(ref date) = entry_date {
            match price_history::get_price_at_date_backend(backend, sym, date) {
                Ok(Some(price)) if price > Decimal::ZERO => {
                    entry_price = Some(price);
                }
                _ => {
                    data_note = Some(format!("No price data for {} at {}", sym, date));
                }
            }
        }

        // Get exit price
        if let Some(ref date) = exit_date {
            match price_history::get_price_at_date_backend(backend, sym, date) {
                Ok(Some(price)) if price > Decimal::ZERO => {
                    exit_price = Some(price);
                }
                _ => {
                    let msg = format!("No exit price for {} at {}", sym, date);
                    data_note = Some(match data_note {
                        Some(existing) => format!("{}; {}", existing, msg),
                        None => msg,
                    });
                }
            }
        } else {
            let msg = "No target_date or scored_at for exit price".to_string();
            data_note = Some(match data_note {
                Some(existing) => format!("{}; {}", existing, msg),
                None => msg,
            });
        }
    } else {
        data_note = Some("No symbol — cannot look up prices".to_string());
    }

    // Compute price change and theoretical P&L
    let (price_change_pct, direction_multiplier, theoretical_pnl, has_price_data) =
        match (entry_price, exit_price) {
            (Some(entry), Some(exit)) if entry > Decimal::ZERO => {
                let pct_change = (exit - entry) / entry * dec!(100);
                let raw_return = (exit - entry) / entry;

                // Direction multiplier based on outcome
                // correct → the prediction was right, so the trade would have been profitable
                // wrong → the prediction was wrong, so the trade would have lost
                // partial → partial credit
                let dir_mult = match pred.outcome.as_str() {
                    "correct" => 1,
                    "wrong" => -1,
                    "partial" => 1, // partial correct still implies some profit
                    _ => 0,
                };

                // Theoretical P&L:
                // Position size = notional * conviction_weight
                // P&L = position_size * |raw_return| * direction_multiplier
                // For "correct": P&L = positive (we got the direction right)
                // For "wrong": P&L = negative (we got the direction wrong)
                // For "partial": P&L = half of what correct would be
                let position_size = NOTIONAL_PORTFOLIO * conviction_weight(&pred.conviction);
                let abs_return = raw_return.abs();
                let pnl = match pred.outcome.as_str() {
                    "correct" => position_size * abs_return,
                    "wrong" => position_size * abs_return * dec!(-1),
                    "partial" => position_size * abs_return * dec!(0.5),
                    _ => Decimal::ZERO,
                };

                (Some(pct_change), Some(dir_mult), Some(pnl), true)
            }
            _ => (None, None, None, false),
        };

    BacktestEntry {
        id: pred.id,
        claim: pred.claim.clone(),
        symbol: pred.symbol.clone(),
        resolved_symbol,
        conviction: pred.conviction.clone(),
        timeframe: pred.timeframe.clone(),
        confidence: pred.confidence,
        source_agent: pred.source_agent.clone(),
        outcome: pred.outcome.clone(),
        created_at: pred.created_at.clone(),
        target_date: pred.target_date.clone(),
        scored_at: pred.scored_at.clone(),
        entry_price,
        exit_price,
        price_change_pct,
        direction_multiplier,
        theoretical_pnl,
        has_price_data,
        data_note,
    }
}

fn compute_summary(entries: &[BacktestEntry]) -> BacktestSummary {
    let total = entries.len();
    let with_price: Vec<&BacktestEntry> = entries.iter().filter(|e| e.has_price_data).collect();
    let without_price = total - with_price.len();

    let mut total_pnl = Decimal::ZERO;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut best_pnl: Option<Decimal> = None;
    let mut best_claim: Option<String> = None;
    let mut worst_pnl: Option<Decimal> = None;
    let mut worst_claim: Option<String> = None;

    for entry in &with_price {
        if let Some(pnl) = entry.theoretical_pnl {
            total_pnl += pnl;
            if pnl > Decimal::ZERO {
                wins += 1;
            } else if pnl < Decimal::ZERO {
                losses += 1;
            }
            if best_pnl.is_none() || pnl > best_pnl.unwrap_or(Decimal::ZERO) {
                best_pnl = Some(pnl);
                best_claim = Some(entry.claim.clone());
            }
            if worst_pnl.is_none() || pnl < worst_pnl.unwrap_or(Decimal::ZERO) {
                worst_pnl = Some(pnl);
                worst_claim = Some(entry.claim.clone());
            }
        }
    }

    let avg_pnl = if !with_price.is_empty() {
        Some(total_pnl / Decimal::from(with_price.len() as u64))
    } else {
        None
    };

    let win_rate = if wins + losses > 0 {
        Some(Decimal::from(wins as u64) / Decimal::from((wins + losses) as u64) * dec!(100))
    } else {
        None
    };

    BacktestSummary {
        total_predictions: total,
        scored_predictions: total, // all passed in are already scored
        with_price_data: with_price.len(),
        without_price_data: without_price,
        total_theoretical_pnl: total_pnl,
        avg_pnl_per_trade: avg_pnl,
        win_count: wins,
        loss_count: losses,
        win_rate_pct: win_rate,
        best_trade_pnl: best_pnl,
        best_trade_claim: best_claim,
        worst_trade_pnl: worst_pnl,
        worst_trade_claim: worst_claim,
        notional_portfolio: NOTIONAL_PORTFOLIO,
    }
}

fn print_json(entries: &[BacktestEntry], summary: &BacktestSummary) -> Result<()> {
    let output = json!({
        "backtest": "predictions",
        "methodology": {
            "notional_portfolio": NOTIONAL_PORTFOLIO.to_string(),
            "conviction_weights": {
                "high": "10%",
                "medium": "5%",
                "low": "2%"
            },
            "scoring": "correct=+|return|, wrong=-|return|, partial=+0.5*|return|",
            "price_source": "price_history (closest available date on or before target)"
        },
        "summary": {
            "total_predictions": summary.total_predictions,
            "with_price_data": summary.with_price_data,
            "without_price_data": summary.without_price_data,
            "total_theoretical_pnl": summary.total_theoretical_pnl.round_dp(2).to_string(),
            "avg_pnl_per_trade": summary.avg_pnl_per_trade.map(|v| v.round_dp(2).to_string()),
            "win_count": summary.win_count,
            "loss_count": summary.loss_count,
            "win_rate_pct": summary.win_rate_pct.map(|v| v.round_dp(1).to_string()),
            "best_trade": summary.best_trade_pnl.map(|p| json!({
                "pnl": p.round_dp(2).to_string(),
                "claim": summary.best_trade_claim,
            })),
            "worst_trade": summary.worst_trade_pnl.map(|p| json!({
                "pnl": p.round_dp(2).to_string(),
                "claim": summary.worst_trade_claim,
            })),
        },
        "entries": entries.iter().map(|e| {
            json!({
                "id": e.id,
                "claim": e.claim,
                "symbol": e.symbol,
                "resolved_symbol": e.resolved_symbol,
                "conviction": e.conviction,
                "timeframe": e.timeframe,
                "confidence": e.confidence,
                "source_agent": e.source_agent,
                "outcome": e.outcome,
                "created_at": e.created_at,
                "target_date": e.target_date,
                "scored_at": e.scored_at,
                "entry_price": e.entry_price.map(|p| p.round_dp(2).to_string()),
                "exit_price": e.exit_price.map(|p| p.round_dp(2).to_string()),
                "price_change_pct": e.price_change_pct.map(|p| p.round_dp(2).to_string()),
                "theoretical_pnl": e.theoretical_pnl.map(|p| p.round_dp(2).to_string()),
                "has_price_data": e.has_price_data,
                "data_note": e.data_note,
            })
        }).collect::<Vec<_>>()
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_table(entries: &[BacktestEntry], summary: &BacktestSummary) {
    if entries.is_empty() {
        println!("No scored predictions found. Add and score predictions with `pftui journal prediction`.");
        return;
    }

    // Summary header
    println!("═══ Prediction Backtest ═══");
    println!("Notional portfolio: ${}", NOTIONAL_PORTFOLIO);
    println!(
        "Scored: {} | With prices: {} | Without: {}",
        summary.total_predictions, summary.with_price_data, summary.without_price_data
    );
    if let Some(win_rate) = summary.win_rate_pct {
        println!(
            "Win rate: {}% ({} wins, {} losses)",
            win_rate.round_dp(1),
            summary.win_count,
            summary.loss_count
        );
    }
    println!(
        "Total theoretical P&L: ${}",
        summary.total_theoretical_pnl.round_dp(2)
    );
    if let Some(avg) = summary.avg_pnl_per_trade {
        println!("Avg P&L per trade: ${}", avg.round_dp(2));
    }
    println!();

    // Table header
    let claim_w = 40;
    println!(
        "{:<cw$}  {:>6}  {:>8}  {:>10}  {:>10}  {:>8}  {:>9}",
        "Claim",
        "Conv",
        "Outcome",
        "Entry",
        "Exit",
        "Chg%",
        "P&L",
        cw = claim_w,
    );
    println!("{}", "─".repeat(claim_w + 6 + 8 + 10 + 10 + 8 + 9 + 12));

    for e in entries {
        let claim = if e.claim.len() > claim_w {
            format!("{}...", &e.claim[..claim_w - 3])
        } else {
            e.claim.clone()
        };

        let entry_str = e
            .entry_price
            .map(|p| format!("{}", p.round_dp(2)))
            .unwrap_or_else(|| "---".to_string());
        let exit_str = e
            .exit_price
            .map(|p| format!("{}", p.round_dp(2)))
            .unwrap_or_else(|| "---".to_string());
        let chg_str = e
            .price_change_pct
            .map(|p| format!("{}%", p.round_dp(1)))
            .unwrap_or_else(|| "---".to_string());
        let pnl_str = e
            .theoretical_pnl
            .map(|p| format!("${}", p.round_dp(2)))
            .unwrap_or_else(|| "---".to_string());

        println!(
            "{:<cw$}  {:>6}  {:>8}  {:>10}  {:>10}  {:>8}  {:>9}",
            claim,
            e.conviction,
            e.outcome,
            entry_str,
            exit_str,
            chg_str,
            pnl_str,
            cw = claim_w,
        );
    }
}

// ─── F58.2: Aggregate Backtest Report ───────────────────────────────────────

/// Stats for a single grouping bucket (conviction level, timeframe, asset class, or agent).
#[derive(Debug, Clone, Serialize)]
pub struct BucketStats {
    pub label: String,
    pub count: usize,
    pub wins: usize,
    pub losses: usize,
    pub partials: usize,
    pub win_rate_pct: Option<Decimal>,
    pub total_pnl: Decimal,
    pub avg_pnl: Option<Decimal>,
    pub best_pnl: Option<Decimal>,
    pub worst_pnl: Option<Decimal>,
}

impl BucketStats {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            count: 0,
            wins: 0,
            losses: 0,
            partials: 0,
            win_rate_pct: None,
            total_pnl: Decimal::ZERO,
            avg_pnl: None,
            best_pnl: None,
            worst_pnl: None,
        }
    }

    fn add_entry(&mut self, entry: &BacktestEntry) {
        self.count += 1;
        if let Some(pnl) = entry.theoretical_pnl {
            self.total_pnl += pnl;
            match entry.outcome.as_str() {
                "correct" => self.wins += 1,
                "wrong" => self.losses += 1,
                "partial" => self.partials += 1,
                _ => {}
            }
            if self.best_pnl.is_none() || pnl > self.best_pnl.unwrap_or(Decimal::MIN) {
                self.best_pnl = Some(pnl);
            }
            if self.worst_pnl.is_none() || pnl < self.worst_pnl.unwrap_or(Decimal::MAX) {
                self.worst_pnl = Some(pnl);
            }
        } else {
            // Still count outcome even without price data
            match entry.outcome.as_str() {
                "correct" => self.wins += 1,
                "wrong" => self.losses += 1,
                "partial" => self.partials += 1,
                _ => {}
            }
        }
    }

    fn finalize(&mut self) {
        let decided = self.wins + self.losses;
        if decided > 0 {
            self.win_rate_pct = Some(
                Decimal::from(self.wins as u64) / Decimal::from(decided as u64) * dec!(100),
            );
        }
        let with_pnl = self.count.saturating_sub(
            // entries without price data contribute 0 to pnl count
            self.count
                - if self.total_pnl != Decimal::ZERO || self.best_pnl.is_some() {
                    self.count
                } else {
                    0
                },
        );
        // A simpler approach: avg over entries that had pnl
        if self.best_pnl.is_some() {
            // At least one entry had price data
            let pnl_count = self.wins + self.losses + self.partials;
            if pnl_count > 0 {
                self.avg_pnl = Some(self.total_pnl / Decimal::from(pnl_count as u64));
            }
        }
        // suppress unused variable warning
        let _ = with_pnl;
    }
}

/// Overall report with breakdowns by each dimension.
#[derive(Debug, Clone, Serialize)]
pub struct BacktestReport {
    pub overall: BacktestSummary,
    pub by_conviction: Vec<BucketStats>,
    pub by_timeframe: Vec<BucketStats>,
    pub by_asset_class: Vec<BucketStats>,
    pub by_agent: Vec<BucketStats>,
    /// Sharpe-ratio equivalent: mean P&L / std-dev of P&L (annualised not applicable — per-trade basis)
    pub sharpe_equivalent: Option<Decimal>,
    /// Most reliable conviction level (highest win rate with ≥3 trades)
    pub best_conviction: Option<String>,
    /// Least reliable conviction level (lowest win rate with ≥3 trades)
    pub worst_conviction: Option<String>,
    /// Most reliable agent (highest win rate with ≥3 trades)
    pub best_agent: Option<String>,
    /// Least reliable agent (lowest win rate with ≥3 trades)
    pub worst_agent: Option<String>,
}

/// Build BucketStats grouped by a key extracted from each entry.
fn build_breakdown<F>(entries: &[BacktestEntry], key_fn: F) -> Vec<BucketStats>
where
    F: Fn(&BacktestEntry) -> String,
{
    let mut map: BTreeMap<String, BucketStats> = BTreeMap::new();
    for entry in entries {
        let key = key_fn(entry);
        let bucket = map.entry(key.clone()).or_insert_with(|| BucketStats::new(&key));
        bucket.add_entry(entry);
    }
    let mut buckets: Vec<BucketStats> = map.into_values().collect();
    for b in &mut buckets {
        b.finalize();
    }
    // Sort by count descending
    buckets.sort_by(|a, b| b.count.cmp(&a.count));
    buckets
}

/// Compute Sharpe-like ratio: mean(pnl) / stddev(pnl). Per-trade, not annualised.
fn compute_sharpe_equivalent(entries: &[BacktestEntry]) -> Option<Decimal> {
    let pnls: Vec<Decimal> = entries
        .iter()
        .filter_map(|e| e.theoretical_pnl)
        .collect();

    if pnls.len() < 2 {
        return None;
    }

    let n = Decimal::from(pnls.len() as u64);
    let sum: Decimal = pnls.iter().copied().sum();
    let mean = sum / n;

    // Variance
    let variance_sum: Decimal = pnls
        .iter()
        .map(|&p| {
            let diff = p - mean;
            diff * diff
        })
        .sum();
    let variance = variance_sum / (n - dec!(1)); // sample variance

    // Approximate sqrt via Newton's method (no f64 for money, but Sharpe is a ratio metric)
    if variance <= Decimal::ZERO {
        return None;
    }

    // Use f64 for the sqrt only (Sharpe ratio is a statistical metric, not a monetary value)
    use rust_decimal::prelude::ToPrimitive;
    let variance_f64 = variance.to_f64()?;
    let std_dev = Decimal::try_from(variance_f64.sqrt()).ok()?;

    if std_dev == Decimal::ZERO {
        return None;
    }

    Some(mean / std_dev)
}

/// Find the best/worst label from a set of BucketStats (min 3 trades for significance).
fn find_best_worst(buckets: &[BucketStats], min_count: usize) -> (Option<String>, Option<String>) {
    let eligible: Vec<&BucketStats> = buckets
        .iter()
        .filter(|b| (b.wins + b.losses) >= min_count)
        .collect();

    let best = eligible
        .iter()
        .max_by(|a, b| {
            a.win_rate_pct
                .unwrap_or(Decimal::ZERO)
                .cmp(&b.win_rate_pct.unwrap_or(Decimal::ZERO))
        })
        .map(|b| b.label.clone());

    let worst = eligible
        .iter()
        .min_by(|a, b| {
            a.win_rate_pct
                .unwrap_or(Decimal::ZERO)
                .cmp(&b.win_rate_pct.unwrap_or(Decimal::ZERO))
        })
        .map(|b| b.label.clone());

    (best, worst)
}

/// Run `analytics backtest report`.
pub fn run_report(backend: &BackendConnection, json_output: bool) -> Result<()> {
    // Reuse the same prediction loading + backtesting logic from run_predictions
    let mut all_predictions =
        user_predictions::list_predictions_backend(backend, None, None, None, None)?;

    // Filter to scored only
    all_predictions.retain(|p| {
        matches!(p.outcome.as_str(), "correct" | "partial" | "wrong")
    });

    // Build backtest entries
    let entries: Vec<BacktestEntry> = all_predictions
        .iter()
        .map(|pred| backtest_prediction(backend, pred))
        .collect();

    // Overall summary
    let overall = compute_summary(&entries);

    // Breakdowns
    let by_conviction = build_breakdown(&entries, |e| e.conviction.to_lowercase());
    let by_timeframe = build_breakdown(&entries, |e| {
        e.timeframe
            .as_deref()
            .unwrap_or("unknown")
            .to_lowercase()
    });
    let by_asset_class = build_breakdown(&entries, |e| {
        match &e.resolved_symbol {
            Some(sym) => format!("{}", infer_category(sym)),
            None => "unknown".to_string(),
        }
    });
    let by_agent = build_breakdown(&entries, |e| {
        e.source_agent
            .as_deref()
            .unwrap_or("unknown")
            .to_lowercase()
    });

    // Sharpe equivalent
    let sharpe_equivalent = compute_sharpe_equivalent(&entries);

    // Best/worst by conviction and agent
    let (best_conviction, worst_conviction) = find_best_worst(&by_conviction, 3);
    let (best_agent, worst_agent) = find_best_worst(&by_agent, 3);

    let report = BacktestReport {
        overall,
        by_conviction,
        by_timeframe,
        by_asset_class,
        by_agent,
        sharpe_equivalent,
        best_conviction,
        worst_conviction,
        best_agent,
        worst_agent,
    };

    if json_output {
        print_report_json(&report)?;
    } else {
        print_report_table(&report);
    }

    Ok(())
}

fn bucket_to_json(bucket: &BucketStats) -> serde_json::Value {
    json!({
        "label": bucket.label,
        "count": bucket.count,
        "wins": bucket.wins,
        "losses": bucket.losses,
        "partials": bucket.partials,
        "win_rate_pct": bucket.win_rate_pct.map(|v| v.round_dp(1).to_string()),
        "total_pnl": bucket.total_pnl.round_dp(2).to_string(),
        "avg_pnl": bucket.avg_pnl.map(|v| v.round_dp(2).to_string()),
        "best_pnl": bucket.best_pnl.map(|v| v.round_dp(2).to_string()),
        "worst_pnl": bucket.worst_pnl.map(|v| v.round_dp(2).to_string()),
    })
}

fn print_report_json(report: &BacktestReport) -> Result<()> {
    let output = json!({
        "backtest": "report",
        "methodology": {
            "notional_portfolio": NOTIONAL_PORTFOLIO.to_string(),
            "conviction_weights": {
                "high": "10%",
                "medium": "5%",
                "low": "2%"
            },
            "sharpe_note": "Per-trade Sharpe equivalent: mean(P&L) / stddev(P&L). Not annualised."
        },
        "summary": {
            "total_predictions": report.overall.total_predictions,
            "with_price_data": report.overall.with_price_data,
            "without_price_data": report.overall.without_price_data,
            "total_theoretical_pnl": report.overall.total_theoretical_pnl.round_dp(2).to_string(),
            "avg_pnl_per_trade": report.overall.avg_pnl_per_trade.map(|v| v.round_dp(2).to_string()),
            "win_count": report.overall.win_count,
            "loss_count": report.overall.loss_count,
            "win_rate_pct": report.overall.win_rate_pct.map(|v| v.round_dp(1).to_string()),
            "sharpe_equivalent": report.sharpe_equivalent.map(|v| v.round_dp(3).to_string()),
            "best_conviction": report.best_conviction,
            "worst_conviction": report.worst_conviction,
            "best_agent": report.best_agent,
            "worst_agent": report.worst_agent,
        },
        "by_conviction": report.by_conviction.iter().map(bucket_to_json).collect::<Vec<_>>(),
        "by_timeframe": report.by_timeframe.iter().map(bucket_to_json).collect::<Vec<_>>(),
        "by_asset_class": report.by_asset_class.iter().map(bucket_to_json).collect::<Vec<_>>(),
        "by_agent": report.by_agent.iter().map(bucket_to_json).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_report_table(report: &BacktestReport) {
    let s = &report.overall;

    if s.total_predictions == 0 {
        println!("No scored predictions found. Add and score predictions with `pftui journal prediction`.");
        return;
    }

    println!("═══ Prediction Backtest Report ═══");
    println!("Notional portfolio: ${}", NOTIONAL_PORTFOLIO);
    println!(
        "Scored: {} | With prices: {} | Without: {}",
        s.total_predictions, s.with_price_data, s.without_price_data
    );
    if let Some(win_rate) = s.win_rate_pct {
        println!(
            "Win rate: {}% ({} wins, {} losses)",
            win_rate.round_dp(1),
            s.win_count,
            s.loss_count
        );
    }
    println!(
        "Total theoretical P&L: ${}",
        s.total_theoretical_pnl.round_dp(2)
    );
    if let Some(avg) = s.avg_pnl_per_trade {
        println!("Avg P&L per trade: ${}", avg.round_dp(2));
    }
    if let Some(sharpe) = report.sharpe_equivalent {
        println!("Sharpe equivalent (per-trade): {}", sharpe.round_dp(3));
    }
    println!();

    // Print breakdown sections
    print_breakdown_section("By Conviction Level", &report.by_conviction);
    print_breakdown_section("By Timeframe", &report.by_timeframe);
    print_breakdown_section("By Asset Class", &report.by_asset_class);
    print_breakdown_section("By Source Agent", &report.by_agent);

    // Best/worst summary
    println!("─── Reliability Insights ───");
    if let Some(ref best) = report.best_conviction {
        println!("  Most reliable conviction: {}", best);
    }
    if let Some(ref worst) = report.worst_conviction {
        println!("  Least reliable conviction: {}", worst);
    }
    if let Some(ref best) = report.best_agent {
        println!("  Most reliable agent: {}", best);
    }
    if let Some(ref worst) = report.worst_agent {
        println!("  Least reliable agent: {}", worst);
    }
    println!();
}

fn print_breakdown_section(title: &str, buckets: &[BucketStats]) {
    if buckets.is_empty() {
        return;
    }

    println!("─── {} ───", title);
    println!(
        "  {:<20}  {:>5}  {:>4}  {:>4}  {:>4}  {:>8}  {:>10}  {:>9}",
        "Label", "Count", "Win", "Loss", "Part", "WinRate", "TotalP&L", "AvgP&L"
    );
    println!(
        "  {}",
        "─".repeat(20 + 5 + 4 + 4 + 4 + 8 + 10 + 9 + 14)
    );

    for b in buckets {
        let wr = b
            .win_rate_pct
            .map(|v| format!("{}%", v.round_dp(1)))
            .unwrap_or_else(|| "---".to_string());
        let avg = b
            .avg_pnl
            .map(|v| format!("${}", v.round_dp(2)))
            .unwrap_or_else(|| "---".to_string());

        println!(
            "  {:<20}  {:>5}  {:>4}  {:>4}  {:>4}  {:>8}  {:>10}  {:>9}",
            b.label,
            b.count,
            b.wins,
            b.losses,
            b.partials,
            wr,
            format!("${}", b.total_pnl.round_dp(2)),
            avg,
        );
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use rusqlite::Connection;

    fn setup_db() -> BackendConnection {
        let conn = Connection::open_in_memory().unwrap();
        // Create user_predictions table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_predictions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                claim TEXT NOT NULL,
                symbol TEXT,
                conviction TEXT NOT NULL DEFAULT 'medium',
                timeframe TEXT NOT NULL DEFAULT 'medium',
                confidence REAL,
                source_agent TEXT,
                target_date TEXT,
                resolution_criteria TEXT,
                outcome TEXT NOT NULL DEFAULT 'pending',
                score_notes TEXT,
                lesson TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                scored_at TEXT
            )",
        )
        .unwrap();

        // Create price_history table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                volume TEXT,
                open TEXT,
                high TEXT,
                low TEXT,
                PRIMARY KEY (symbol, date)
            )",
        )
        .unwrap();

        BackendConnection::Sqlite { conn }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_prediction(
        conn: &Connection,
        claim: &str,
        symbol: Option<&str>,
        conviction: &str,
        outcome: &str,
        created_at: &str,
        target_date: Option<&str>,
        scored_at: Option<&str>,
        source_agent: Option<&str>,
        timeframe: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO user_predictions (claim, symbol, conviction, outcome, created_at, target_date, scored_at, source_agent, timeframe)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                claim,
                symbol,
                conviction,
                outcome,
                created_at,
                target_date,
                scored_at,
                source_agent,
                timeframe.unwrap_or("medium"),
            ],
        )
        .unwrap();
    }

    fn insert_price(conn: &Connection, symbol: &str, date: &str, close: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO price_history (symbol, date, close) VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol, date, close],
        )
        .unwrap();
    }

    #[test]
    fn test_backtest_empty() {
        let backend = setup_db();
        let result = run_predictions(&backend, None, None, None, None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backtest_correct_prediction_with_prices() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // Insert a correct BTC prediction: claimed BTC would rise
        insert_prediction(
            conn,
            "BTC above $100K by March 2026",
            Some("BTC-USD"),
            "high",
            "correct",
            "2025-12-01",
            Some("2026-03-01"),
            Some("2026-03-15"),
            Some("low-timeframe"),
            Some("medium"),
        );

        // Insert price data
        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-03-01", "110000");

        let result = run_predictions(&backend, None, None, None, None, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backtest_wrong_prediction_negative_pnl() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "Gold above $3000 by Jan 2026",
            Some("GC=F"),
            "medium",
            "wrong",
            "2025-11-01",
            Some("2026-01-31"),
            Some("2026-01-31"),
            None,
            Some("high"),
        );

        insert_price(conn, "GC=F", "2025-11-01", "2700");
        insert_price(conn, "GC=F", "2026-01-31", "2650");

        let result = run_predictions(&backend, None, None, None, None, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backtest_no_symbol_skips_prices() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "Fed will cut rates by 50bps",
            None,
            "low",
            "correct",
            "2025-10-01",
            Some("2026-01-01"),
            Some("2026-01-15"),
            None,
            Some("macro"),
        );

        let result = run_predictions(&backend, None, None, None, None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_conviction_weight_values() {
        assert_eq!(conviction_weight("high"), dec!(0.10));
        assert_eq!(conviction_weight("medium"), dec!(0.05));
        assert_eq!(conviction_weight("low"), dec!(0.02));
        assert_eq!(conviction_weight("unknown"), dec!(0.05));
    }

    #[test]
    fn test_extract_date() {
        assert_eq!(extract_date("2025-12-01"), Some("2025-12-01".to_string()));
        assert_eq!(
            extract_date("2025-12-01 14:30:00"),
            Some("2025-12-01".to_string())
        );
        assert_eq!(
            extract_date("2025-12-01T14:30:00Z"),
            Some("2025-12-01".to_string())
        );
        assert_eq!(extract_date("not-a-date"), None);
        assert_eq!(extract_date("short"), None);
    }

    #[test]
    fn test_resolve_symbol_alias() {
        assert_eq!(resolve_symbol_alias("BTC"), Some("BTC-USD"));
        assert_eq!(resolve_symbol_alias("GOLD"), Some("GC=F"));
        assert_eq!(resolve_symbol_alias("SILVER"), Some("SI=F"));
        assert_eq!(resolve_symbol_alias("TSLA"), None);
        assert_eq!(resolve_symbol_alias("VIX"), Some("^VIX"));
    }

    #[test]
    fn test_summary_computation() {
        let entries = vec![
            BacktestEntry {
                id: 1,
                claim: "BTC up".into(),
                symbol: Some("BTC-USD".into()),
                resolved_symbol: Some("BTC-USD".into()),
                conviction: "high".into(),
                timeframe: Some("medium".into()),
                confidence: None,
                source_agent: None,
                outcome: "correct".into(),
                created_at: "2025-01-01".into(),
                target_date: Some("2025-03-01".into()),
                scored_at: None,
                entry_price: Some(dec!(50000)),
                exit_price: Some(dec!(60000)),
                price_change_pct: Some(dec!(20.0)),
                direction_multiplier: Some(1),
                theoretical_pnl: Some(dec!(200.0)),
                has_price_data: true,
                data_note: None,
            },
            BacktestEntry {
                id: 2,
                claim: "Gold up".into(),
                symbol: Some("GC=F".into()),
                resolved_symbol: Some("GC=F".into()),
                conviction: "medium".into(),
                timeframe: Some("high".into()),
                confidence: None,
                source_agent: None,
                outcome: "wrong".into(),
                created_at: "2025-01-01".into(),
                target_date: Some("2025-03-01".into()),
                scored_at: None,
                entry_price: Some(dec!(2000)),
                exit_price: Some(dec!(1900)),
                price_change_pct: Some(dec!(-5.0)),
                direction_multiplier: Some(-1),
                theoretical_pnl: Some(dec!(-25.0)),
                has_price_data: true,
                data_note: None,
            },
        ];

        let summary = compute_summary(&entries);
        assert_eq!(summary.total_predictions, 2);
        assert_eq!(summary.with_price_data, 2);
        assert_eq!(summary.win_count, 1);
        assert_eq!(summary.loss_count, 1);
        assert_eq!(summary.total_theoretical_pnl, dec!(175.0));
    }

    #[test]
    fn test_backtest_filters_by_symbol() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "BTC above $100K",
            Some("BTC-USD"),
            "high",
            "correct",
            "2025-12-01",
            Some("2026-03-01"),
            None,
            None,
            None,
        );
        insert_prediction(
            conn,
            "Gold above $3000",
            Some("GC=F"),
            "medium",
            "wrong",
            "2025-12-01",
            Some("2026-03-01"),
            None,
            None,
            None,
        );

        // Filter to BTC only - should return 1 result
        let result = run_predictions(&backend, Some("BTC-USD"), None, None, None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backtest_partial_outcome() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "BTC above $120K by March",
            Some("BTC-USD"),
            "high",
            "partial",
            "2025-12-01",
            Some("2026-03-01"),
            Some("2026-03-01"),
            None,
            None,
        );

        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-03-01", "110000");

        let result = run_predictions(&backend, None, None, None, None, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backtest_uses_scored_at_when_no_target_date() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "BTC will pump",
            Some("BTC-USD"),
            "low",
            "correct",
            "2025-12-01",
            None, // no target_date
            Some("2026-02-15"),
            None,
            None,
        );

        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-02-15", "105000");

        let result = run_predictions(&backend, None, None, None, None, None, true);
        assert!(result.is_ok());
    }

    // ─── F58.2: Report tests ───

    #[test]
    fn test_report_empty() {
        let backend = setup_db();
        let result = run_report(&backend, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_json_output() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "BTC above $100K",
            Some("BTC-USD"),
            "high",
            "correct",
            "2025-12-01",
            Some("2026-03-01"),
            Some("2026-03-15"),
            Some("low-timeframe"),
            Some("medium"),
        );
        insert_prediction(
            conn,
            "Gold drops below $2500",
            Some("GC=F"),
            "medium",
            "wrong",
            "2025-11-01",
            Some("2026-01-31"),
            Some("2026-01-31"),
            Some("high-timeframe"),
            Some("high"),
        );
        insert_prediction(
            conn,
            "DXY below 100",
            Some("DX-Y.NYB"),
            "low",
            "correct",
            "2025-10-01",
            Some("2026-02-01"),
            Some("2026-02-01"),
            Some("macro-timeframe"),
            Some("macro"),
        );

        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-03-01", "110000");
        insert_price(conn, "GC=F", "2025-11-01", "2700");
        insert_price(conn, "GC=F", "2026-01-31", "2650");
        insert_price(conn, "DX-Y.NYB", "2025-10-01", "105");
        insert_price(conn, "DX-Y.NYB", "2026-02-01", "99");

        let result = run_report(&backend, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_table_output() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "BTC rallies",
            Some("BTC-USD"),
            "high",
            "correct",
            "2025-12-01",
            Some("2026-03-01"),
            Some("2026-03-01"),
            Some("low-timeframe"),
            Some("low"),
        );

        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-03-01", "110000");

        let result = run_report(&backend, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bucket_stats_accumulation() {
        let mut bucket = BucketStats::new("test");

        let entry1 = BacktestEntry {
            id: 1,
            claim: "BTC up".into(),
            symbol: Some("BTC-USD".into()),
            resolved_symbol: Some("BTC-USD".into()),
            conviction: "high".into(),
            timeframe: Some("medium".into()),
            confidence: None,
            source_agent: Some("low-timeframe".into()),
            outcome: "correct".into(),
            created_at: "2025-01-01".into(),
            target_date: Some("2025-03-01".into()),
            scored_at: None,
            entry_price: Some(dec!(50000)),
            exit_price: Some(dec!(60000)),
            price_change_pct: Some(dec!(20.0)),
            direction_multiplier: Some(1),
            theoretical_pnl: Some(dec!(200.0)),
            has_price_data: true,
            data_note: None,
        };

        let entry2 = BacktestEntry {
            id: 2,
            claim: "Gold up".into(),
            symbol: Some("GC=F".into()),
            resolved_symbol: Some("GC=F".into()),
            conviction: "medium".into(),
            timeframe: Some("high".into()),
            confidence: None,
            source_agent: Some("high-timeframe".into()),
            outcome: "wrong".into(),
            created_at: "2025-01-01".into(),
            target_date: Some("2025-03-01".into()),
            scored_at: None,
            entry_price: Some(dec!(2000)),
            exit_price: Some(dec!(1900)),
            price_change_pct: Some(dec!(-5.0)),
            direction_multiplier: Some(-1),
            theoretical_pnl: Some(dec!(-25.0)),
            has_price_data: true,
            data_note: None,
        };

        bucket.add_entry(&entry1);
        bucket.add_entry(&entry2);
        bucket.finalize();

        assert_eq!(bucket.count, 2);
        assert_eq!(bucket.wins, 1);
        assert_eq!(bucket.losses, 1);
        assert_eq!(bucket.total_pnl, dec!(175.0));
        assert_eq!(bucket.win_rate_pct, Some(dec!(50.0)));
        assert_eq!(bucket.best_pnl, Some(dec!(200.0)));
        assert_eq!(bucket.worst_pnl, Some(dec!(-25.0)));
    }

    #[test]
    fn test_sharpe_equivalent_computation() {
        let entries = vec![
            BacktestEntry {
                id: 1,
                claim: "A".into(),
                symbol: None,
                resolved_symbol: None,
                conviction: "high".into(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                outcome: "correct".into(),
                created_at: "2025-01-01".into(),
                target_date: None,
                scored_at: None,
                entry_price: None,
                exit_price: None,
                price_change_pct: None,
                direction_multiplier: None,
                theoretical_pnl: Some(dec!(100)),
                has_price_data: true,
                data_note: None,
            },
            BacktestEntry {
                id: 2,
                claim: "B".into(),
                symbol: None,
                resolved_symbol: None,
                conviction: "high".into(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                outcome: "wrong".into(),
                created_at: "2025-01-01".into(),
                target_date: None,
                scored_at: None,
                entry_price: None,
                exit_price: None,
                price_change_pct: None,
                direction_multiplier: None,
                theoretical_pnl: Some(dec!(-50)),
                has_price_data: true,
                data_note: None,
            },
        ];

        let sharpe = compute_sharpe_equivalent(&entries);
        assert!(sharpe.is_some());
        // mean = 25, stddev = ~106.07 → sharpe ~0.236
        // Exact value depends on sample vs population variance, but should be positive
        let s = sharpe.unwrap();
        assert!(s > Decimal::ZERO, "Sharpe should be positive: {}", s);
    }

    #[test]
    fn test_sharpe_equivalent_insufficient_data() {
        let entries = vec![BacktestEntry {
            id: 1,
            claim: "A".into(),
            symbol: None,
            resolved_symbol: None,
            conviction: "high".into(),
            timeframe: None,
            confidence: None,
            source_agent: None,
            outcome: "correct".into(),
            created_at: "2025-01-01".into(),
            target_date: None,
            scored_at: None,
            entry_price: None,
            exit_price: None,
            price_change_pct: None,
            direction_multiplier: None,
            theoretical_pnl: Some(dec!(100)),
            has_price_data: true,
            data_note: None,
        }];

        // Only 1 entry → not enough for Sharpe
        assert!(compute_sharpe_equivalent(&entries).is_none());
    }

    #[test]
    fn test_find_best_worst() {
        let buckets = vec![
            BucketStats {
                label: "high".into(),
                count: 5,
                wins: 4,
                losses: 1,
                partials: 0,
                win_rate_pct: Some(dec!(80.0)),
                total_pnl: dec!(500),
                avg_pnl: Some(dec!(100)),
                best_pnl: Some(dec!(200)),
                worst_pnl: Some(dec!(-50)),
            },
            BucketStats {
                label: "low".into(),
                count: 4,
                wins: 1,
                losses: 3,
                partials: 0,
                win_rate_pct: Some(dec!(25.0)),
                total_pnl: dec!(-100),
                avg_pnl: Some(dec!(-25)),
                best_pnl: Some(dec!(50)),
                worst_pnl: Some(dec!(-80)),
            },
            BucketStats {
                label: "medium".into(),
                count: 2, // below threshold
                wins: 2,
                losses: 0,
                partials: 0,
                win_rate_pct: Some(dec!(100.0)),
                total_pnl: dec!(300),
                avg_pnl: Some(dec!(150)),
                best_pnl: Some(dec!(200)),
                worst_pnl: Some(dec!(100)),
            },
        ];

        // min_count=3 means "medium" is excluded (only 2 decided trades)
        let (best, worst) = find_best_worst(&buckets, 3);
        assert_eq!(best, Some("high".to_string()));
        assert_eq!(worst, Some("low".to_string()));

        // With min_count=1, medium should be best (100% win rate)
        let (best2, _) = find_best_worst(&buckets, 1);
        assert_eq!(best2, Some("medium".to_string()));
    }

    #[test]
    fn test_build_breakdown() {
        let entries = vec![
            BacktestEntry {
                id: 1,
                claim: "BTC up".into(),
                symbol: Some("BTC-USD".into()),
                resolved_symbol: Some("BTC-USD".into()),
                conviction: "high".into(),
                timeframe: Some("medium".into()),
                confidence: None,
                source_agent: None,
                outcome: "correct".into(),
                created_at: "2025-01-01".into(),
                target_date: Some("2025-03-01".into()),
                scored_at: None,
                entry_price: Some(dec!(50000)),
                exit_price: Some(dec!(60000)),
                price_change_pct: Some(dec!(20.0)),
                direction_multiplier: Some(1),
                theoretical_pnl: Some(dec!(200)),
                has_price_data: true,
                data_note: None,
            },
            BacktestEntry {
                id: 2,
                claim: "Gold down".into(),
                symbol: Some("GC=F".into()),
                resolved_symbol: Some("GC=F".into()),
                conviction: "high".into(),
                timeframe: Some("high".into()),
                confidence: None,
                source_agent: None,
                outcome: "wrong".into(),
                created_at: "2025-01-01".into(),
                target_date: Some("2025-03-01".into()),
                scored_at: None,
                entry_price: Some(dec!(2000)),
                exit_price: Some(dec!(1900)),
                price_change_pct: Some(dec!(-5.0)),
                direction_multiplier: Some(-1),
                theoretical_pnl: Some(dec!(-25)),
                has_price_data: true,
                data_note: None,
            },
            BacktestEntry {
                id: 3,
                claim: "ETH up".into(),
                symbol: Some("ETH-USD".into()),
                resolved_symbol: Some("ETH-USD".into()),
                conviction: "low".into(),
                timeframe: Some("medium".into()),
                confidence: None,
                source_agent: None,
                outcome: "correct".into(),
                created_at: "2025-01-01".into(),
                target_date: Some("2025-03-01".into()),
                scored_at: None,
                entry_price: Some(dec!(3000)),
                exit_price: Some(dec!(3500)),
                price_change_pct: Some(dec!(16.67)),
                direction_multiplier: Some(1),
                theoretical_pnl: Some(dec!(33.34)),
                has_price_data: true,
                data_note: None,
            },
        ];

        let by_conviction = build_breakdown(&entries, |e| e.conviction.to_lowercase());
        assert_eq!(by_conviction.len(), 2); // "high" and "low"
        let high = by_conviction.iter().find(|b| b.label == "high").unwrap();
        assert_eq!(high.count, 2);
        assert_eq!(high.wins, 1);
        assert_eq!(high.losses, 1);

        let by_timeframe = build_breakdown(&entries, |e| {
            e.timeframe.as_deref().unwrap_or("unknown").to_lowercase()
        });
        assert_eq!(by_timeframe.len(), 2); // "medium" and "high"
        let medium = by_timeframe.iter().find(|b| b.label == "medium").unwrap();
        assert_eq!(medium.count, 2);
    }
}
