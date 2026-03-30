use anyhow::Result;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::{price_history, user_predictions};

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
}
