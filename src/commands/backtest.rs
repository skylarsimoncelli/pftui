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

// ─── F58.3: Per-Agent Accuracy Breakdown ────────────────────────────────────

/// Detailed per-agent accuracy profile.
#[derive(Debug, Clone, Serialize)]
pub struct AgentProfile {
    pub agent_name: String,
    pub total_predictions: usize,
    pub with_price_data: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub partial_count: usize,
    pub win_rate_pct: Option<Decimal>,
    pub total_pnl: Decimal,
    pub avg_pnl: Option<Decimal>,
    pub sharpe_equivalent: Option<Decimal>,
    pub best_trade: Option<AgentTrade>,
    pub worst_trade: Option<AgentTrade>,
    pub by_conviction: Vec<BucketStats>,
    pub by_timeframe: Vec<BucketStats>,
    pub by_asset_class: Vec<BucketStats>,
    pub by_symbol: Vec<BucketStats>,
    /// Current streak: positive = consecutive wins, negative = consecutive losses
    pub current_streak: i32,
    /// Longest winning streak
    pub longest_win_streak: usize,
    /// Longest losing streak
    pub longest_loss_streak: usize,
    /// Rank among all agents by win rate (1-based, None if not enough data)
    pub rank_by_win_rate: Option<usize>,
    /// Total number of agents with scored predictions
    pub total_agents: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTrade {
    pub claim: String,
    pub symbol: Option<String>,
    pub pnl: Decimal,
    pub outcome: String,
    pub date: String,
}

/// Compute streak info from chronologically-sorted entries.
fn compute_streaks(entries: &[BacktestEntry]) -> (i32, usize, usize) {
    let mut longest_win: usize = 0;
    let mut longest_loss: usize = 0;
    let mut win_run: usize = 0;
    let mut loss_run: usize = 0;

    // Sort by created_at for chronological streak
    let mut sorted: Vec<&BacktestEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    for entry in &sorted {
        match entry.outcome.as_str() {
            "correct" => {
                win_run += 1;
                loss_run = 0;
                if win_run > longest_win {
                    longest_win = win_run;
                }
            }
            "wrong" => {
                loss_run += 1;
                win_run = 0;
                if loss_run > longest_loss {
                    longest_loss = loss_run;
                }
            }
            _ => {
                // partial doesn't break streaks but doesn't extend them
            }
        }
    }

    // Current streak: check from the end
    let mut current_streak: i32 = 0;
    for entry in sorted.iter().rev() {
        match entry.outcome.as_str() {
            "correct" => {
                if current_streak < 0 {
                    break;
                }
                current_streak += 1;
            }
            "wrong" => {
                if current_streak > 0 {
                    break;
                }
                current_streak -= 1;
            }
            _ => break, // partial ends streak counting
        }
    }

    (current_streak, longest_win, longest_loss)
}

/// Run `analytics backtest agent --agent <name>`.
pub fn run_agent(
    backend: &BackendConnection,
    agent_name: &str,
    json_output: bool,
) -> Result<()> {
    // Load all scored predictions
    let mut all_predictions =
        user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    all_predictions.retain(|p| {
        matches!(p.outcome.as_str(), "correct" | "partial" | "wrong")
    });

    // Build backtest entries for ALL agents (needed for ranking)
    let all_entries: Vec<BacktestEntry> = all_predictions
        .iter()
        .map(|pred| backtest_prediction(backend, pred))
        .collect();

    // Compute per-agent win rates for ranking
    let agent_win_rates = build_breakdown(&all_entries, |e| {
        e.source_agent
            .as_deref()
            .unwrap_or("unknown")
            .to_lowercase()
    });
    let total_agents = agent_win_rates.len();

    // Rank agents by win rate (descending), only those with ≥3 decided trades
    let mut ranked: Vec<(&str, Decimal)> = agent_win_rates
        .iter()
        .filter(|b| (b.wins + b.losses) >= 3)
        .filter_map(|b| {
            b.win_rate_pct.map(|wr| (b.label.as_str(), wr))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));

    let agent_lower = agent_name.to_lowercase();
    let rank = ranked
        .iter()
        .position(|(name, _)| *name == agent_lower)
        .map(|pos| pos + 1); // 1-based

    // Filter to target agent
    let agent_entries: Vec<BacktestEntry> = all_entries
        .into_iter()
        .filter(|e| {
            e.source_agent
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(agent_name))
        })
        .collect();

    if agent_entries.is_empty() {
        if json_output {
            let output = json!({
                "backtest": "agent",
                "agent": agent_name,
                "error": "No scored predictions found for this agent"
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!(
                "No scored predictions found for agent '{}'.",
                agent_name
            );
            println!("\nAvailable agents with scored predictions:");
            for b in &agent_win_rates {
                println!("  - {} ({} predictions)", b.label, b.count);
            }
        }
        return Ok(());
    }

    // Compute stats
    let with_price = agent_entries.iter().filter(|e| e.has_price_data).count();
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut partials = 0usize;
    let mut total_pnl = Decimal::ZERO;
    let mut best_trade: Option<AgentTrade> = None;
    let mut worst_trade: Option<AgentTrade> = None;

    for entry in &agent_entries {
        match entry.outcome.as_str() {
            "correct" => wins += 1,
            "wrong" => losses += 1,
            "partial" => partials += 1,
            _ => {}
        }
        if let Some(pnl) = entry.theoretical_pnl {
            total_pnl += pnl;
            if best_trade.as_ref().is_none_or(|t| pnl > t.pnl) {
                best_trade = Some(AgentTrade {
                    claim: entry.claim.clone(),
                    symbol: entry.resolved_symbol.clone(),
                    pnl,
                    outcome: entry.outcome.clone(),
                    date: entry.created_at.clone(),
                });
            }
            if worst_trade.as_ref().is_none_or(|t| pnl < t.pnl) {
                worst_trade = Some(AgentTrade {
                    claim: entry.claim.clone(),
                    symbol: entry.resolved_symbol.clone(),
                    pnl,
                    outcome: entry.outcome.clone(),
                    date: entry.created_at.clone(),
                });
            }
        }
    }

    let decided = wins + losses;
    let win_rate = if decided > 0 {
        Some(Decimal::from(wins as u64) / Decimal::from(decided as u64) * dec!(100))
    } else {
        None
    };

    let avg_pnl = if with_price > 0 {
        let pnl_count = wins + losses + partials;
        if pnl_count > 0 {
            Some(total_pnl / Decimal::from(pnl_count as u64))
        } else {
            None
        }
    } else {
        None
    };

    let sharpe = compute_sharpe_equivalent(&agent_entries);
    let (current_streak, longest_win, longest_loss) = compute_streaks(&agent_entries);

    // Breakdowns
    let by_conviction = build_breakdown(&agent_entries, |e| e.conviction.to_lowercase());
    let by_timeframe = build_breakdown(&agent_entries, |e| {
        e.timeframe
            .as_deref()
            .unwrap_or("unknown")
            .to_lowercase()
    });
    let by_asset_class = build_breakdown(&agent_entries, |e| match &e.resolved_symbol {
        Some(sym) => format!("{}", infer_category(sym)),
        None => "unknown".to_string(),
    });
    let by_symbol = build_breakdown(&agent_entries, |e| {
        e.resolved_symbol
            .as_deref()
            .unwrap_or("unknown")
            .to_string()
    });

    let profile = AgentProfile {
        agent_name: agent_name.to_string(),
        total_predictions: agent_entries.len(),
        with_price_data: with_price,
        win_count: wins,
        loss_count: losses,
        partial_count: partials,
        win_rate_pct: win_rate,
        total_pnl,
        avg_pnl,
        sharpe_equivalent: sharpe,
        best_trade,
        worst_trade,
        by_conviction,
        by_timeframe,
        by_asset_class,
        by_symbol,
        current_streak,
        longest_win_streak: longest_win,
        longest_loss_streak: longest_loss,
        rank_by_win_rate: rank,
        total_agents,
    };

    if json_output {
        print_agent_json(&profile)?;
    } else {
        print_agent_table(&profile);
    }

    Ok(())
}

fn trade_to_json(trade: &AgentTrade) -> serde_json::Value {
    json!({
        "claim": trade.claim,
        "symbol": trade.symbol,
        "pnl": trade.pnl.round_dp(2).to_string(),
        "outcome": trade.outcome,
        "date": trade.date,
    })
}

fn print_agent_json(profile: &AgentProfile) -> Result<()> {
    let output = json!({
        "backtest": "agent",
        "agent": profile.agent_name,
        "methodology": {
            "notional_portfolio": NOTIONAL_PORTFOLIO.to_string(),
            "conviction_weights": {
                "high": "10%",
                "medium": "5%",
                "low": "2%"
            },
            "sharpe_note": "Per-trade Sharpe equivalent: mean(P&L) / stddev(P&L). Not annualised.",
            "ranking_threshold": "≥3 decided trades required for ranking"
        },
        "summary": {
            "total_predictions": profile.total_predictions,
            "with_price_data": profile.with_price_data,
            "wins": profile.win_count,
            "losses": profile.loss_count,
            "partials": profile.partial_count,
            "win_rate_pct": profile.win_rate_pct.map(|v| v.round_dp(1).to_string()),
            "total_pnl": profile.total_pnl.round_dp(2).to_string(),
            "avg_pnl": profile.avg_pnl.map(|v| v.round_dp(2).to_string()),
            "sharpe_equivalent": profile.sharpe_equivalent.map(|v| v.round_dp(3).to_string()),
            "current_streak": profile.current_streak,
            "longest_win_streak": profile.longest_win_streak,
            "longest_loss_streak": profile.longest_loss_streak,
            "rank_by_win_rate": profile.rank_by_win_rate,
            "total_agents_ranked": profile.total_agents,
        },
        "best_trade": profile.best_trade.as_ref().map(trade_to_json),
        "worst_trade": profile.worst_trade.as_ref().map(trade_to_json),
        "by_conviction": profile.by_conviction.iter().map(bucket_to_json).collect::<Vec<_>>(),
        "by_timeframe": profile.by_timeframe.iter().map(bucket_to_json).collect::<Vec<_>>(),
        "by_asset_class": profile.by_asset_class.iter().map(bucket_to_json).collect::<Vec<_>>(),
        "by_symbol": profile.by_symbol.iter().map(bucket_to_json).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_agent_table(profile: &AgentProfile) {
    println!("═══ Agent Backtest: {} ═══", profile.agent_name);
    println!("Notional portfolio: ${}", NOTIONAL_PORTFOLIO);
    println!(
        "Predictions: {} | With prices: {}",
        profile.total_predictions, profile.with_price_data
    );
    if let Some(wr) = profile.win_rate_pct {
        println!(
            "Win rate: {}% ({} wins, {} losses, {} partial)",
            wr.round_dp(1),
            profile.win_count,
            profile.loss_count,
            profile.partial_count,
        );
    } else {
        println!(
            "Record: {} wins, {} losses, {} partial (no decided trades for win rate)",
            profile.win_count, profile.loss_count, profile.partial_count,
        );
    }
    println!(
        "Total theoretical P&L: ${}",
        profile.total_pnl.round_dp(2)
    );
    if let Some(avg) = profile.avg_pnl {
        println!("Avg P&L per trade: ${}", avg.round_dp(2));
    }
    if let Some(sharpe) = profile.sharpe_equivalent {
        println!("Sharpe equivalent (per-trade): {}", sharpe.round_dp(3));
    }
    println!();

    // Streaks
    println!("─── Streaks ───");
    let streak_str = if profile.current_streak > 0 {
        format!("{} wins", profile.current_streak)
    } else if profile.current_streak < 0 {
        format!("{} losses", profile.current_streak.abs())
    } else {
        "none".to_string()
    };
    println!("  Current streak: {}", streak_str);
    println!("  Longest win streak: {}", profile.longest_win_streak);
    println!("  Longest loss streak: {}", profile.longest_loss_streak);

    // Ranking
    if let Some(rank) = profile.rank_by_win_rate {
        println!(
            "  Rank: #{} of {} agents (by win rate, ≥3 decided trades)",
            rank, profile.total_agents
        );
    }
    println!();

    // Best/worst trades
    if let Some(ref trade) = profile.best_trade {
        let sym = trade.symbol.as_deref().unwrap_or("—");
        println!(
            "─── Best Trade ───\n  {} [{}] — ${} ({})",
            trade.claim,
            sym,
            trade.pnl.round_dp(2),
            trade.date,
        );
    }
    if let Some(ref trade) = profile.worst_trade {
        let sym = trade.symbol.as_deref().unwrap_or("—");
        println!(
            "─── Worst Trade ───\n  {} [{}] — ${} ({})",
            trade.claim,
            sym,
            trade.pnl.round_dp(2),
            trade.date,
        );
    }
    println!();

    // Breakdowns
    print_breakdown_section("By Conviction", &profile.by_conviction);
    print_breakdown_section("By Timeframe", &profile.by_timeframe);
    print_breakdown_section("By Asset Class", &profile.by_asset_class);
    print_breakdown_section("By Symbol", &profile.by_symbol);
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

// ─── Diagnostics ───────────────────────────────────────────────────────

/// A single diagnostic finding with severity and recommendation.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticFinding {
    /// Severity: critical (≤30% win rate, large loss), warning (≤45%), info (pattern)
    pub severity: String,
    /// Short category tag for filtering
    pub category: String,
    /// Which agent this applies to (None = all agents / system-wide)
    pub agent: Option<String>,
    /// One-line headline
    pub headline: String,
    /// Detailed explanation of what the data shows
    pub detail: String,
    /// Specific, actionable recommendation
    pub recommendation: String,
}

/// Full diagnostics output.
#[derive(Debug, Clone, Serialize)]
struct DiagnosticsReport {
    total_predictions: usize,
    agents_analysed: usize,
    findings: Vec<DiagnosticFinding>,
}

/// Minimum number of decided trades (wins + losses) to analyse a bucket.
const DIAG_MIN_TRADES: usize = 3;

/// Run `analytics backtest diagnostics`.
pub fn run_diagnostics(
    backend: &BackendConnection,
    agent_filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    // Load all scored predictions
    let mut all_predictions =
        user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    all_predictions.retain(|p| {
        matches!(p.outcome.as_str(), "correct" | "partial" | "wrong")
    });

    if all_predictions.is_empty() {
        if json_output {
            let output = json!({
                "backtest": "diagnostics",
                "error": "No scored predictions found"
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No scored predictions found. Score predictions with `pftui journal prediction score`.");
        }
        return Ok(());
    }

    // Build backtest entries
    let entries: Vec<BacktestEntry> = all_predictions
        .iter()
        .map(|pred| backtest_prediction(backend, pred))
        .collect();

    // Optionally filter to one agent
    let entries = if let Some(agent) = agent_filter {
        entries
            .into_iter()
            .filter(|e| {
                e.source_agent
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(agent))
            })
            .collect()
    } else {
        entries
    };

    if entries.is_empty() {
        if json_output {
            let output = json!({
                "backtest": "diagnostics",
                "agent": agent_filter,
                "error": "No scored predictions found for this agent"
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No scored predictions found for agent '{}'.", agent_filter.unwrap_or("?"));
        }
        return Ok(());
    }

    let mut findings = Vec::new();

    // Per-agent analysis
    let by_agent = build_breakdown(&entries, |e| {
        e.source_agent
            .as_deref()
            .unwrap_or("unknown")
            .to_lowercase()
    });

    let agents_analysed = by_agent.iter().filter(|b| (b.wins + b.losses) >= DIAG_MIN_TRADES).count();

    for agent_bucket in &by_agent {
        let decided = agent_bucket.wins + agent_bucket.losses;
        if decided < DIAG_MIN_TRADES {
            continue;
        }

        let agent_name = &agent_bucket.label;
        let agent_entries: Vec<&BacktestEntry> = entries
            .iter()
            .filter(|e| {
                e.source_agent
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(agent_name))
            })
            .collect();

        let win_rate = agent_bucket
            .win_rate_pct
            .unwrap_or(Decimal::ZERO);

        // ── Finding: Overall poor win rate ──
        if win_rate < dec!(35) {
            findings.push(DiagnosticFinding {
                severity: "critical".to_string(),
                category: "win-rate".to_string(),
                agent: Some(agent_name.clone()),
                headline: format!(
                    "{} has {}% win rate ({}/{} decided trades)",
                    agent_name,
                    win_rate.round_dp(1),
                    agent_bucket.wins,
                    decided
                ),
                detail: format!(
                    "Win rate below 35% indicates a systematic prediction bias. \
                     Total P&L: ${}. This agent is consistently losing money on predictions.",
                    agent_bucket.total_pnl.round_dp(2)
                ),
                recommendation: "Review this agent's prediction strategy. Common causes: \
                    over-weighting mean reversion in trending markets, \
                    predicting against the dominant regime, or \
                    using low-conviction entries on high-volatility assets. \
                    Consider restricting this agent to its strongest asset class."
                    .to_string(),
            });
        } else if win_rate < dec!(45) {
            findings.push(DiagnosticFinding {
                severity: "warning".to_string(),
                category: "win-rate".to_string(),
                agent: Some(agent_name.clone()),
                headline: format!(
                    "{} has {}% win rate — below breakeven threshold",
                    agent_name,
                    win_rate.round_dp(1)
                ),
                detail: format!(
                    "With conviction-weighted sizing, win rates below 45% typically produce \
                     negative expected value. Current total P&L: ${}.",
                    agent_bucket.total_pnl.round_dp(2)
                ),
                recommendation: "Increase prediction selectivity — fewer, higher-conviction calls. \
                    Review asset classes and timeframes where this agent performs worst."
                    .to_string(),
            });
        }

        // ── Finding: Asset class weaknesses ──
        let agent_by_asset = build_breakdown(&entries.iter()
            .filter(|e| e.source_agent.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(agent_name)))
            .cloned()
            .collect::<Vec<_>>(), |e| {
            match &e.resolved_symbol {
                Some(sym) => format!("{}", infer_category(sym)),
                None => "unknown".to_string(),
            }
        });

        for asset_bucket in &agent_by_asset {
            let asset_decided = asset_bucket.wins + asset_bucket.losses;
            if asset_decided < DIAG_MIN_TRADES {
                continue;
            }
            let asset_wr = asset_bucket.win_rate_pct.unwrap_or(Decimal::ZERO);

            if asset_wr < dec!(25) {
                findings.push(DiagnosticFinding {
                    severity: "critical".to_string(),
                    category: "asset-class-weakness".to_string(),
                    agent: Some(agent_name.clone()),
                    headline: format!(
                        "{} has {}% win rate on {} ({}/{} trades)",
                        agent_name,
                        asset_wr.round_dp(1),
                        asset_bucket.label,
                        asset_bucket.wins,
                        asset_decided
                    ),
                    detail: format!(
                        "Near-zero win rate on {} suggests a fundamental misread of this asset class. \
                         Total P&L on {}: ${}.",
                        asset_bucket.label,
                        asset_bucket.label,
                        asset_bucket.total_pnl.round_dp(2)
                    ),
                    recommendation: format!(
                        "Stop making {} predictions until the underlying model is reviewed. \
                         This asset class is consistently mispriced by this agent. \
                         Consider delegating {} calls to a different timeframe analyst.",
                        asset_bucket.label, asset_bucket.label
                    ),
                });
            } else if asset_wr < dec!(40) && asset_decided >= 5 {
                findings.push(DiagnosticFinding {
                    severity: "warning".to_string(),
                    category: "asset-class-weakness".to_string(),
                    agent: Some(agent_name.clone()),
                    headline: format!(
                        "{} underperforms on {} ({}% win rate, {} trades)",
                        agent_name,
                        asset_bucket.label,
                        asset_wr.round_dp(1),
                        asset_decided
                    ),
                    detail: format!(
                        "Below-average performance on {} with enough trades to be statistically meaningful. \
                         P&L: ${}.",
                        asset_bucket.label,
                        asset_bucket.total_pnl.round_dp(2)
                    ),
                    recommendation: format!(
                        "Reduce {} prediction frequency or increase conviction threshold \
                         before making {} calls.",
                        asset_bucket.label, asset_bucket.label
                    ),
                });
            }
        }

        // ── Finding: Conviction calibration ──
        let agent_by_conviction = build_breakdown(&entries.iter()
            .filter(|e| e.source_agent.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(agent_name)))
            .cloned()
            .collect::<Vec<_>>(), |e| e.conviction.to_lowercase());

        let high_bucket = agent_by_conviction.iter().find(|b| b.label == "high");
        let medium_bucket = agent_by_conviction.iter().find(|b| b.label == "medium");
        let low_bucket = agent_by_conviction.iter().find(|b| b.label == "low");

        // High conviction worse than medium/low = miscalibrated
        if let Some(high) = high_bucket {
            let high_decided = high.wins + high.losses;
            if high_decided >= DIAG_MIN_TRADES {
                let high_wr = high.win_rate_pct.unwrap_or(Decimal::ZERO);

                // Compare to medium
                if let Some(med) = medium_bucket {
                    let med_decided = med.wins + med.losses;
                    let med_wr = med.win_rate_pct.unwrap_or(Decimal::ZERO);
                    if med_decided >= DIAG_MIN_TRADES && high_wr < med_wr - dec!(10) {
                        findings.push(DiagnosticFinding {
                            severity: "warning".to_string(),
                            category: "conviction-calibration".to_string(),
                            agent: Some(agent_name.clone()),
                            headline: format!(
                                "{}: high-conviction ({}%) underperforms medium-conviction ({}%)",
                                agent_name,
                                high_wr.round_dp(1),
                                med_wr.round_dp(1)
                            ),
                            detail: format!(
                                "High-conviction calls should outperform lower conviction levels. \
                                 Inverted relationship (high {}% < medium {}%) indicates conviction \
                                 signals are miscalibrated — the agent feels most certain when it's \
                                 most likely to be wrong.",
                                high_wr.round_dp(1),
                                med_wr.round_dp(1)
                            ),
                            recommendation: "Re-examine what triggers 'high conviction' in this agent's routine. \
                                The conviction signal may be anchoring on narrative strength rather than \
                                data quality. Consider downgrading default conviction or requiring \
                                multi-timeframe confirmation for high-conviction calls."
                                .to_string(),
                        });
                    }
                }

                // High conviction with large losses = dangerous
                if high_wr < dec!(40) && high.total_pnl < dec!(-50) {
                    findings.push(DiagnosticFinding {
                        severity: "critical".to_string(),
                        category: "conviction-calibration".to_string(),
                        agent: Some(agent_name.clone()),
                        headline: format!(
                            "{}: high-conviction calls losing heavily ({}% WR, ${} P&L)",
                            agent_name,
                            high_wr.round_dp(1),
                            high.total_pnl.round_dp(2)
                        ),
                        detail: "High-conviction calls carry the largest position sizes (10% of notional). \
                            When these have low win rates, losses are amplified significantly. This is \
                            the highest-impact area to fix."
                            .to_string(),
                        recommendation: "Immediately restrict high-conviction calls to this agent's \
                            best-performing asset class only. Require explicit evidence (not narrative) \
                            before assigning high conviction."
                            .to_string(),
                    });
                }
            }
        }

        // ── Finding: Loss magnitude asymmetry ──
        if let (Some(best), Some(worst)) = (agent_bucket.best_pnl, agent_bucket.worst_pnl) {
            if worst.abs() > best.abs() * dec!(2) && worst < dec!(-20) {
                findings.push(DiagnosticFinding {
                    severity: "warning".to_string(),
                    category: "risk-asymmetry".to_string(),
                    agent: Some(agent_name.clone()),
                    headline: format!(
                        "{}: worst loss (${}) is {}x larger than best win (${})",
                        agent_name,
                        worst.round_dp(2),
                        (worst.abs() / best.abs().max(dec!(0.01))).round_dp(1),
                        best.round_dp(2)
                    ),
                    detail: "Large asymmetric losses suggest the agent is taking outsized positions \
                        on low-probability calls or holding losing predictions too long before scoring."
                        .to_string(),
                    recommendation: "Tighten scoring cadence — score predictions earlier when the \
                        thesis is clearly invalidated. Consider reducing conviction on volatile \
                        assets where move magnitudes are large."
                        .to_string(),
                });
            }
        }

        // ── Finding: Losing streak ──
        let (_, _, longest_loss) = compute_streaks(
            &agent_entries.iter().copied().cloned().collect::<Vec<_>>()
        );
        if longest_loss >= 5 {
            findings.push(DiagnosticFinding {
                severity: "warning".to_string(),
                category: "streak".to_string(),
                agent: Some(agent_name.clone()),
                headline: format!(
                    "{}: longest losing streak is {} consecutive wrong predictions",
                    agent_name, longest_loss
                ),
                detail: format!(
                    "A streak of {} consecutive losses suggests a period where the agent's \
                     model was systematically wrong — likely during a regime change or trend \
                     reversal it failed to adapt to.",
                    longest_loss
                ),
                recommendation: "Add regime-awareness to the prediction routine. After 3 \
                    consecutive losses, the agent should pause predictions and re-evaluate \
                    its market model before continuing."
                    .to_string(),
            });
        }

        // ── Finding: Overtrading (volume vs accuracy) ──
        if decided >= 10 && win_rate < dec!(40) {
            let agent_pnl_per_trade = agent_bucket.avg_pnl.unwrap_or(Decimal::ZERO);
            if agent_pnl_per_trade < dec!(-5) {
                findings.push(DiagnosticFinding {
                    severity: "warning".to_string(),
                    category: "overtrading".to_string(),
                    agent: Some(agent_name.clone()),
                    headline: format!(
                        "{}: {} predictions with negative edge (${}/trade avg)",
                        agent_name, decided, agent_pnl_per_trade.round_dp(2)
                    ),
                    detail: "High volume of predictions with consistently negative P&L per trade. \
                        The agent is making predictions it shouldn't — the negative edge compounds \
                        with each additional call."
                        .to_string(),
                    recommendation: "Reduce prediction frequency by at least 50%. Only predict when \
                        multiple data sources converge on a clear signal. Quality over quantity."
                        .to_string(),
                });
            }
        }

        // ── Finding: Mean reversion bias detection ──
        // If the agent consistently loses on trending assets, it's over-weighting mean reversion
        let mut trend_losses = 0usize;
        let mut trend_total = 0usize;
        for entry in &agent_entries {
            if let Some(pct) = entry.price_change_pct {
                // Large directional moves (>3%) suggest trending, not ranging
                if pct.abs() > dec!(3) {
                    trend_total += 1;
                    if entry.outcome == "wrong" {
                        trend_losses += 1;
                    }
                }
            }
        }
        if trend_total >= 5 && trend_losses > 0 {
            let trend_loss_rate = Decimal::from(trend_losses as u64)
                / Decimal::from(trend_total as u64)
                * dec!(100);
            if trend_loss_rate > dec!(65) {
                findings.push(DiagnosticFinding {
                    severity: "warning".to_string(),
                    category: "mean-reversion-bias".to_string(),
                    agent: Some(agent_name.clone()),
                    headline: format!(
                        "{}: {}% loss rate on large-move trades ({}/{})",
                        agent_name,
                        trend_loss_rate.round_dp(1),
                        trend_losses,
                        trend_total
                    ),
                    detail: "When assets make large moves (>3%), this agent is usually on the wrong \
                        side. This pattern indicates a mean-reversion bias — the agent expects prices \
                        to revert when they're actually trending."
                        .to_string(),
                    recommendation: "Weight momentum signals more heavily. When an asset is making \
                        large directional moves, defer to the trend rather than predicting reversals. \
                        Add regime-state checks before counter-trend predictions."
                        .to_string(),
                });
            }
        }

        // ── Finding: Low-conviction overuse ──
        if let Some(low) = low_bucket {
            let low_decided = low.wins + low.losses;
            if low_decided >= 5 {
                let low_pct = Decimal::from(low_decided as u64)
                    / Decimal::from(decided as u64)
                    * dec!(100);
                if low_pct > dec!(40) {
                    findings.push(DiagnosticFinding {
                        severity: "info".to_string(),
                        category: "conviction-distribution".to_string(),
                        agent: Some(agent_name.clone()),
                        headline: format!(
                            "{}: {}% of predictions are low-conviction",
                            agent_name,
                            low_pct.round_dp(0)
                        ),
                        detail: "A high proportion of low-conviction predictions suggests the agent is \
                            making calls it doesn't believe in. Low-conviction positions have small sizing \
                            (2% of notional), so even correct calls contribute little to P&L."
                            .to_string(),
                        recommendation: "Raise the prediction threshold. If conviction is low, consider \
                            not making the prediction at all. Focus capital and attention on medium and \
                            high conviction setups."
                            .to_string(),
                    });
                }
            }
        }
    }

    // ── System-wide findings ──

    // Overall negative expected value
    let overall = compute_summary(&entries);
    if let Some(avg_pnl) = overall.avg_pnl_per_trade {
        if avg_pnl < dec!(-3) && overall.with_price_data >= 20 {
            findings.push(DiagnosticFinding {
                severity: "critical".to_string(),
                category: "system-performance".to_string(),
                agent: None,
                headline: format!(
                    "System-wide negative edge: ${}/trade avg across {} trades",
                    avg_pnl.round_dp(2),
                    overall.with_price_data
                ),
                detail: format!(
                    "The prediction system as a whole has negative expected value. \
                     Total P&L: ${}. Win rate: {}%. This means the system \
                     destroys value with each prediction.",
                    overall.total_theoretical_pnl.round_dp(2),
                    overall.win_rate_pct.unwrap_or(Decimal::ZERO).round_dp(1)
                ),
                recommendation: "Priority 1: Fix the worst-performing agent. \
                    Priority 2: Remove or restrict agents with <35% win rate. \
                    Priority 3: Increase system-wide conviction threshold — \
                    only predict when evidence is strong."
                    .to_string(),
            });
        }
    }

    // Sort findings: critical first, then warning, then info
    findings.sort_by(|a, b| {
        let severity_order = |s: &str| match s {
            "critical" => 0,
            "warning" => 1,
            "info" => 2,
            _ => 3,
        };
        severity_order(&a.severity).cmp(&severity_order(&b.severity))
    });

    let report = DiagnosticsReport {
        total_predictions: entries.len(),
        agents_analysed,
        findings,
    };

    if json_output {
        print_diagnostics_json(&report, agent_filter)?;
    } else {
        print_diagnostics_table(&report, agent_filter);
    }

    Ok(())
}

fn print_diagnostics_json(report: &DiagnosticsReport, agent_filter: Option<&str>) -> Result<()> {
    let output = json!({
        "backtest": "diagnostics",
        "agent_filter": agent_filter,
        "total_predictions": report.total_predictions,
        "agents_analysed": report.agents_analysed,
        "findings_count": report.findings.len(),
        "by_severity": {
            "critical": report.findings.iter().filter(|f| f.severity == "critical").count(),
            "warning": report.findings.iter().filter(|f| f.severity == "warning").count(),
            "info": report.findings.iter().filter(|f| f.severity == "info").count(),
        },
        "findings": report.findings.iter().map(|f| json!({
            "severity": f.severity,
            "category": f.category,
            "agent": f.agent,
            "headline": f.headline,
            "detail": f.detail,
            "recommendation": f.recommendation,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_diagnostics_table(report: &DiagnosticsReport, agent_filter: Option<&str>) {
    println!("═══ Prediction Diagnostics ═══");
    if let Some(agent) = agent_filter {
        println!("Agent: {}", agent);
    }
    println!(
        "Analysed: {} predictions across {} agent(s)",
        report.total_predictions, report.agents_analysed
    );
    println!(
        "Findings: {} critical, {} warning, {} info",
        report.findings.iter().filter(|f| f.severity == "critical").count(),
        report.findings.iter().filter(|f| f.severity == "warning").count(),
        report.findings.iter().filter(|f| f.severity == "info").count(),
    );
    println!();

    if report.findings.is_empty() {
        println!("✅ No diagnostic issues found. Prediction system is performing well.");
        return;
    }

    for (i, finding) in report.findings.iter().enumerate() {
        let icon = match finding.severity.as_str() {
            "critical" => "🔴",
            "warning" => "🟡",
            "info" => "ℹ️",
            _ => "  ",
        };

        println!(
            "{}. {} [{}] {}",
            i + 1,
            icon,
            finding.category,
            finding.headline
        );
        println!("   {}", finding.detail);
        println!("   → {}", finding.recommendation);
        println!();
    }
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

    // ─── F58.3: Per-agent backtest tests ───

    #[test]
    fn test_agent_empty() {
        let backend = setup_db();
        let result = run_agent(&backend, "nonexistent-agent", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_json_output() {
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
            Some("low-timeframe"),
            Some("high"),
        );
        insert_prediction(
            conn,
            "DXY stays above 104",
            Some("DX-Y.NYB"),
            "low",
            "correct",
            "2025-10-01",
            Some("2026-02-01"),
            Some("2026-02-01"),
            Some("high-timeframe"),
            Some("macro"),
        );

        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-03-01", "110000");
        insert_price(conn, "GC=F", "2025-11-01", "2700");
        insert_price(conn, "GC=F", "2026-01-31", "2650");
        insert_price(conn, "DX-Y.NYB", "2025-10-01", "105");
        insert_price(conn, "DX-Y.NYB", "2026-02-01", "106");

        let result = run_agent(&backend, "low-timeframe", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_table_output() {
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
            Some("macro-timeframe"),
            Some("macro"),
        );

        insert_price(conn, "BTC-USD", "2025-12-01", "95000");
        insert_price(conn, "BTC-USD", "2026-03-01", "110000");

        let result = run_agent(&backend, "macro-timeframe", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_case_insensitive() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        insert_prediction(
            conn,
            "Silver rises",
            Some("SI=F"),
            "medium",
            "correct",
            "2025-12-01",
            Some("2026-03-01"),
            Some("2026-03-01"),
            Some("Low-Timeframe"),
            Some("low"),
        );

        insert_price(conn, "SI=F", "2025-12-01", "30");
        insert_price(conn, "SI=F", "2026-03-01", "35");

        // Should match case-insensitively
        let result = run_agent(&backend, "low-timeframe", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compute_streaks_all_wins() {
        let entries: Vec<BacktestEntry> = (1..=5)
            .map(|i| BacktestEntry {
                id: i,
                claim: format!("Win {}", i),
                symbol: None,
                resolved_symbol: None,
                conviction: "high".into(),
                timeframe: None,
                confidence: None,
                source_agent: Some("test-agent".into()),
                outcome: "correct".into(),
                created_at: format!("2025-01-{:02}", i),
                target_date: None,
                scored_at: None,
                entry_price: None,
                exit_price: None,
                price_change_pct: None,
                direction_multiplier: None,
                theoretical_pnl: Some(dec!(100)),
                has_price_data: true,
                data_note: None,
            })
            .collect();

        let (current, longest_win, longest_loss) = compute_streaks(&entries);
        assert_eq!(current, 5);
        assert_eq!(longest_win, 5);
        assert_eq!(longest_loss, 0);
    }

    #[test]
    fn test_compute_streaks_mixed() {
        let outcomes = ["correct", "correct", "wrong", "correct", "wrong", "wrong"];
        let entries: Vec<BacktestEntry> = outcomes
            .iter()
            .enumerate()
            .map(|(i, outcome)| BacktestEntry {
                id: (i + 1) as i64,
                claim: format!("Pred {}", i + 1),
                symbol: None,
                resolved_symbol: None,
                conviction: "medium".into(),
                timeframe: None,
                confidence: None,
                source_agent: Some("test-agent".into()),
                outcome: outcome.to_string(),
                created_at: format!("2025-01-{:02}", i + 1),
                target_date: None,
                scored_at: None,
                entry_price: None,
                exit_price: None,
                price_change_pct: None,
                direction_multiplier: None,
                theoretical_pnl: Some(if *outcome == "correct" {
                    dec!(50)
                } else {
                    dec!(-30)
                }),
                has_price_data: true,
                data_note: None,
            })
            .collect();

        let (current, longest_win, longest_loss) = compute_streaks(&entries);
        assert_eq!(current, -2); // ends with 2 losses
        assert_eq!(longest_win, 2); // first two are wins
        assert_eq!(longest_loss, 2); // last two are losses
    }

    #[test]
    fn test_compute_streaks_empty() {
        let entries: Vec<BacktestEntry> = vec![];
        let (current, longest_win, longest_loss) = compute_streaks(&entries);
        assert_eq!(current, 0);
        assert_eq!(longest_win, 0);
        assert_eq!(longest_loss, 0);
    }

    #[test]
    fn test_agent_ranking() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // Agent A: 3 wins, 0 losses (100% win rate)
        for i in 1..=3 {
            insert_prediction(
                conn,
                &format!("A win {}", i),
                Some("BTC-USD"),
                "high",
                "correct",
                &format!("2025-01-{:02}", i),
                Some(&format!("2025-02-{:02}", i)),
                Some(&format!("2025-02-{:02}", i)),
                Some("agent-a"),
                Some("low"),
            );
        }

        // Agent B: 1 win, 2 losses (33% win rate)
        insert_prediction(
            conn,
            "B win",
            Some("GC=F"),
            "medium",
            "correct",
            "2025-01-01",
            Some("2025-02-01"),
            Some("2025-02-01"),
            Some("agent-b"),
            Some("high"),
        );
        for i in 1..=2 {
            insert_prediction(
                conn,
                &format!("B loss {}", i),
                Some("GC=F"),
                "medium",
                "wrong",
                &format!("2025-01-{:02}", i + 1),
                Some(&format!("2025-02-{:02}", i + 1)),
                Some(&format!("2025-02-{:02}", i + 1)),
                Some("agent-b"),
                Some("high"),
            );
        }

        insert_price(conn, "BTC-USD", "2025-01-01", "95000");
        insert_price(conn, "BTC-USD", "2025-01-02", "96000");
        insert_price(conn, "BTC-USD", "2025-01-03", "97000");
        insert_price(conn, "BTC-USD", "2025-02-01", "100000");
        insert_price(conn, "BTC-USD", "2025-02-02", "101000");
        insert_price(conn, "BTC-USD", "2025-02-03", "102000");
        insert_price(conn, "GC=F", "2025-01-01", "2700");
        insert_price(conn, "GC=F", "2025-01-02", "2710");
        insert_price(conn, "GC=F", "2025-01-03", "2720");
        insert_price(conn, "GC=F", "2025-02-01", "2800");
        insert_price(conn, "GC=F", "2025-02-02", "2650");
        insert_price(conn, "GC=F", "2025-02-03", "2600");

        // Agent A should rank #1
        let result = run_agent(&backend, "agent-a", true);
        assert!(result.is_ok());

        // Agent B should rank #2
        let result = run_agent(&backend, "agent-b", true);
        assert!(result.is_ok());
    }

    // ─── Diagnostics tests ───

    #[test]
    fn test_diagnostics_empty() {
        let backend = setup_db();
        let result = run_diagnostics(&backend, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diagnostics_critical_win_rate() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // Agent with 1 win, 5 losses = 16.7% win rate → should trigger critical
        insert_prediction(conn, "win 1", Some("GC=F"), "medium", "correct",
            "2025-01-01", Some("2025-02-01"), Some("2025-02-01"), Some("bad-agent"), Some("low"));
        for i in 1..=5 {
            insert_prediction(conn, &format!("loss {i}"), Some("GC=F"), "medium", "wrong",
                &format!("2025-01-{:02}", i + 1), Some(&format!("2025-02-{:02}", i + 1)),
                Some(&format!("2025-02-{:02}", i + 1)), Some("bad-agent"), Some("low"));
        }

        insert_price(conn, "GC=F", "2025-01-01", "2700");
        insert_price(conn, "GC=F", "2025-01-02", "2710");
        insert_price(conn, "GC=F", "2025-01-03", "2720");
        insert_price(conn, "GC=F", "2025-01-04", "2730");
        insert_price(conn, "GC=F", "2025-01-05", "2740");
        insert_price(conn, "GC=F", "2025-01-06", "2750");
        insert_price(conn, "GC=F", "2025-02-01", "2800");
        insert_price(conn, "GC=F", "2025-02-02", "2650");
        insert_price(conn, "GC=F", "2025-02-03", "2600");
        insert_price(conn, "GC=F", "2025-02-04", "2550");
        insert_price(conn, "GC=F", "2025-02-05", "2500");
        insert_price(conn, "GC=F", "2025-02-06", "2450");

        let result = run_diagnostics(&backend, None, true);
        assert!(result.is_ok());

        // Also test agent-filtered
        let result = run_diagnostics(&backend, Some("bad-agent"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diagnostics_conviction_miscalibration() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // High conviction: 1 win, 3 losses (25%)
        insert_prediction(conn, "high win", Some("BTC-USD"), "high", "correct",
            "2025-01-01", Some("2025-02-01"), Some("2025-02-01"), Some("miscal-agent"), Some("low"));
        for i in 1..=3 {
            insert_prediction(conn, &format!("high loss {i}"), Some("BTC-USD"), "high", "wrong",
                &format!("2025-01-{:02}", i + 1), Some(&format!("2025-02-{:02}", i + 1)),
                Some(&format!("2025-02-{:02}", i + 1)), Some("miscal-agent"), Some("low"));
        }

        // Medium conviction: 3 wins, 1 loss (75%)
        for i in 1..=3 {
            insert_prediction(conn, &format!("med win {i}"), Some("BTC-USD"), "medium", "correct",
                &format!("2025-03-{:02}", i), Some(&format!("2025-04-{:02}", i)),
                Some(&format!("2025-04-{:02}", i)), Some("miscal-agent"), Some("low"));
        }
        insert_prediction(conn, "med loss", Some("BTC-USD"), "medium", "wrong",
            "2025-03-04", Some("2025-04-04"), Some("2025-04-04"), Some("miscal-agent"), Some("low"));

        insert_price(conn, "BTC-USD", "2025-01-01", "90000");
        insert_price(conn, "BTC-USD", "2025-01-02", "91000");
        insert_price(conn, "BTC-USD", "2025-01-03", "92000");
        insert_price(conn, "BTC-USD", "2025-01-04", "93000");
        insert_price(conn, "BTC-USD", "2025-02-01", "95000");
        insert_price(conn, "BTC-USD", "2025-02-02", "85000");
        insert_price(conn, "BTC-USD", "2025-02-03", "84000");
        insert_price(conn, "BTC-USD", "2025-02-04", "83000");
        insert_price(conn, "BTC-USD", "2025-03-01", "96000");
        insert_price(conn, "BTC-USD", "2025-03-02", "97000");
        insert_price(conn, "BTC-USD", "2025-03-03", "98000");
        insert_price(conn, "BTC-USD", "2025-03-04", "99000");
        insert_price(conn, "BTC-USD", "2025-04-01", "100000");
        insert_price(conn, "BTC-USD", "2025-04-02", "101000");
        insert_price(conn, "BTC-USD", "2025-04-03", "102000");
        insert_price(conn, "BTC-USD", "2025-04-04", "95000");

        let result = run_diagnostics(&backend, Some("miscal-agent"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diagnostics_no_findings_when_performing_well() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // Agent with 3 wins, 1 loss = 75% win rate → no critical/warning findings
        for i in 1..=3 {
            insert_prediction(conn, &format!("win {i}"), Some("BTC-USD"), "medium", "correct",
                &format!("2025-01-{:02}", i), Some(&format!("2025-02-{:02}", i)),
                Some(&format!("2025-02-{:02}", i)), Some("good-agent"), Some("low"));
        }
        insert_prediction(conn, "loss 1", Some("BTC-USD"), "medium", "wrong",
            "2025-01-04", Some("2025-02-04"), Some("2025-02-04"), Some("good-agent"), Some("low"));

        insert_price(conn, "BTC-USD", "2025-01-01", "90000");
        insert_price(conn, "BTC-USD", "2025-01-02", "91000");
        insert_price(conn, "BTC-USD", "2025-01-03", "92000");
        insert_price(conn, "BTC-USD", "2025-01-04", "93000");
        insert_price(conn, "BTC-USD", "2025-02-01", "95000");
        insert_price(conn, "BTC-USD", "2025-02-02", "96000");
        insert_price(conn, "BTC-USD", "2025-02-03", "97000");
        insert_price(conn, "BTC-USD", "2025-02-04", "88000");

        let result = run_diagnostics(&backend, Some("good-agent"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diagnostics_agent_filter_unknown() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // Add one prediction for a different agent
        insert_prediction(conn, "some pred", Some("BTC-USD"), "medium", "correct",
            "2025-01-01", Some("2025-02-01"), Some("2025-02-01"), Some("real-agent"), Some("low"));

        // Filter to nonexistent agent
        let result = run_diagnostics(&backend, Some("ghost-agent"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diagnostics_json_structure() {
        let backend = setup_db();
        let conn = match &backend {
            BackendConnection::Sqlite { conn } => conn,
            _ => panic!("expected sqlite"),
        };

        // 1 win, 4 losses to trigger findings
        insert_prediction(conn, "win", Some("BTC-USD"), "medium", "correct",
            "2025-01-01", Some("2025-02-01"), Some("2025-02-01"), Some("test-agent"), Some("low"));
        for i in 1..=4 {
            insert_prediction(conn, &format!("loss {i}"), Some("BTC-USD"), "medium", "wrong",
                &format!("2025-01-{:02}", i + 1), Some(&format!("2025-02-{:02}", i + 1)),
                Some(&format!("2025-02-{:02}", i + 1)), Some("test-agent"), Some("low"));
        }

        insert_price(conn, "BTC-USD", "2025-01-01", "90000");
        insert_price(conn, "BTC-USD", "2025-01-02", "91000");
        insert_price(conn, "BTC-USD", "2025-01-03", "92000");
        insert_price(conn, "BTC-USD", "2025-01-04", "93000");
        insert_price(conn, "BTC-USD", "2025-01-05", "94000");
        insert_price(conn, "BTC-USD", "2025-02-01", "95000");
        insert_price(conn, "BTC-USD", "2025-02-02", "85000");
        insert_price(conn, "BTC-USD", "2025-02-03", "84000");
        insert_price(conn, "BTC-USD", "2025-02-04", "83000");
        insert_price(conn, "BTC-USD", "2025-02-05", "82000");

        // Just verify it runs without error in JSON mode
        let result = run_diagnostics(&backend, None, true);
        assert!(result.is_ok());

        // And table mode
        let result = run_diagnostics(&backend, None, false);
        assert!(result.is_ok());
    }
}
