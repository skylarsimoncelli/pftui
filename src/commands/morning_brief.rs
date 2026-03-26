//! `pftui analytics morning-brief` — Consolidated morning intelligence command.
//!
//! Combines portfolio brief, regime, scenarios, situations, correlation breaks,
//! movers, news sentiment, and recent alerts into a single JSON payload for
//! agent consumption. Reduces agent startup from 5-8 separate CLI calls to one.

use anyhow::Result;
use rust_decimal::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

use crate::config::Config;
use crate::db::alerts as db_alerts;
use crate::db::backend::BackendConnection;
use crate::db::news_cache::get_latest_news_backend;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_prices_at_date_backend;
use crate::db::regime_snapshots;
use crate::db::scenarios;

/// Run the `analytics morning-brief` command.
pub fn run(backend: &BackendConnection, config: &Config, hours: i64, json: bool) -> Result<()> {
    if json {
        run_json(backend, config, hours)
    } else {
        run_human(backend, config, hours)
    }
}

// ── Data structures ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MorningBrief {
    timestamp: String,
    regime: Option<RegimeSummary>,
    portfolio: PortfolioSnapshot,
    scenarios: Vec<ScenarioSummary>,
    situations: Vec<SituationSummary>,
    correlation_breaks: Vec<CorrelationBreakSummary>,
    movers: Vec<MoverSummary>,
    news_sentiment: Vec<SentimentCategorySummary>,
    recent_alerts: Vec<AlertSummary>,
}

#[derive(Debug, Serialize)]
struct RegimeSummary {
    regime: String,
    confidence: Option<f64>,
    drivers: Option<String>,
    vix: Option<f64>,
    dxy: Option<f64>,
    yield_10y: Option<f64>,
    oil: Option<f64>,
    gold: Option<f64>,
    btc: Option<f64>,
    recorded_at: String,
}

#[derive(Debug, Serialize)]
struct PortfolioSnapshot {
    total_positions: usize,
    prices: Vec<PriceEntry>,
}

#[derive(Debug, Serialize)]
struct PriceEntry {
    symbol: String,
    price: String,
    change_pct: Option<String>,
}

#[derive(Debug, Serialize)]
struct ScenarioSummary {
    name: String,
    probability: f64,
    status: String,
    phase: String,
    description: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct SituationSummary {
    id: i64,
    name: String,
    probability: f64,
    phase: String,
    branch_count: usize,
    impact_count: usize,
    indicator_count: usize,
    indicators_triggered: usize,
    update_count: usize,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct CorrelationBreakSummary {
    symbol_a: String,
    symbol_b: String,
    corr_7d: Option<f64>,
    corr_90d: Option<f64>,
    break_delta: f64,
}

#[derive(Debug, Serialize)]
struct MoverSummary {
    symbol: String,
    price: String,
    change_pct: String,
}

#[derive(Debug, Serialize)]
struct SentimentCategorySummary {
    category: String,
    count: usize,
    avg_score: f64,
    label: String,
    bullish: usize,
    bearish: usize,
    neutral: usize,
}

#[derive(Debug, Serialize)]
struct AlertSummary {
    id: i64,
    symbol: String,
    rule_text: String,
    status: String,
    triggered_at: Option<String>,
}

// ── JSON output ──────────────────────────────────────────────────────────────

fn run_json(backend: &BackendConnection, config: &Config, hours: i64) -> Result<()> {
    let brief = build_brief(backend, config, hours)?;
    println!("{}", serde_json::to_string_pretty(&brief)?);
    Ok(())
}

// ── Human-readable output ────────────────────────────────────────────────────

fn run_human(backend: &BackendConnection, config: &Config, hours: i64) -> Result<()> {
    let brief = build_brief(backend, config, hours)?;

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    MORNING BRIEF                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Regime
    if let Some(ref regime) = brief.regime {
        println!("┌─ REGIME ─────────────────────────────────────────────────────┐");
        println!(
            "│ {} (confidence: {:.0}%)",
            regime.regime.to_uppercase(),
            regime.confidence.unwrap_or(0.0) * 100.0
        );
        if let Some(ref d) = regime.drivers {
            println!("│ Drivers: {}", d);
        }
        println!(
            "│ VIX: {}  DXY: {}  10Y: {}  Oil: {}  Gold: {}  BTC: {}",
            fmt_opt_f64(regime.vix),
            fmt_opt_f64(regime.dxy),
            fmt_opt_f64(regime.yield_10y),
            fmt_opt_f64(regime.oil),
            fmt_opt_f64(regime.gold),
            fmt_opt_f64(regime.btc),
        );
        println!("└───────────────────────────────────────────────────────────────┘\n");
    }

    // Scenarios
    if !brief.scenarios.is_empty() {
        println!("┌─ SCENARIOS ──────────────────────────────────────────────────┐");
        for s in &brief.scenarios {
            let desc = s
                .description
                .as_ref()
                .map(|d| {
                    if d.len() > 50 {
                        format!(" — {}...", &d[..47])
                    } else {
                        format!(" — {}", d)
                    }
                })
                .unwrap_or_default();
            println!("│ {:25} {:5.1}%  [{}]{}", s.name, s.probability, s.status, desc);
        }
        println!("└───────────────────────────────────────────────────────────────┘\n");
    }

    // Situations
    if !brief.situations.is_empty() {
        println!("┌─ ACTIVE SITUATIONS ──────────────────────────────────────────┐");
        for sit in &brief.situations {
            let ind = if sit.indicator_count > 0 {
                format!(
                    " | ind: {}/{}",
                    sit.indicators_triggered, sit.indicator_count
                )
            } else {
                String::new()
            };
            println!(
                "│ [{}] {} ({:.1}%)  {}b {}i {}u{}",
                sit.id,
                sit.name,
                sit.probability,
                sit.branch_count,
                sit.impact_count,
                sit.update_count,
                ind,
            );
        }
        println!("└───────────────────────────────────────────────────────────────┘\n");
    }

    // Movers
    if !brief.movers.is_empty() {
        println!("┌─ TOP MOVERS ─────────────────────────────────────────────────┐");
        for m in &brief.movers {
            println!("│ {:12} {:>12}  {:>8}", m.symbol, m.price, m.change_pct);
        }
        println!("└───────────────────────────────────────────────────────────────┘\n");
    }

    // Correlation Breaks
    if !brief.correlation_breaks.is_empty() {
        println!("┌─ CORRELATION BREAKS ─────────────────────────────────────────┐");
        for cb in &brief.correlation_breaks {
            println!(
                "│ {}-{}  7d:{} 90d:{}  Δ{:+.2}",
                cb.symbol_a,
                cb.symbol_b,
                fmt_opt_f64(cb.corr_7d),
                fmt_opt_f64(cb.corr_90d),
                cb.break_delta,
            );
        }
        println!("└───────────────────────────────────────────────────────────────┘\n");
    }

    // News Sentiment
    if !brief.news_sentiment.is_empty() {
        println!("┌─ NEWS SENTIMENT ─────────────────────────────────────────────┐");
        for ns in &brief.news_sentiment {
            println!(
                "│ {:20} {:>5.1}  {} ({} articles)",
                ns.category, ns.avg_score, ns.label, ns.count
            );
        }
        println!("└───────────────────────────────────────────────────────────────┘\n");
    }

    // Recent Alerts
    if !brief.recent_alerts.is_empty() {
        println!("┌─ RECENT ALERTS ──────────────────────────────────────────────┐");
        for a in &brief.recent_alerts {
            let ts = a
                .triggered_at
                .as_deref()
                .unwrap_or("--");
            println!("│ {} [{}] {} ({})", a.symbol, a.status, a.rule_text, ts);
        }
        println!("└───────────────────────────────────────────────────────────────┘");
    }

    Ok(())
}

// ── Builder ──────────────────────────────────────────────────────────────────

fn build_brief(
    backend: &BackendConnection,
    _config: &Config,
    hours: i64,
) -> Result<MorningBrief> {
    let timestamp = chrono::Utc::now().to_rfc3339();

    // 1. Regime
    let regime = build_regime(backend)?;

    // 2. Portfolio snapshot (cached prices + daily changes)
    let portfolio = build_portfolio(backend)?;

    // 3. Scenarios (all active)
    let scenarios = build_scenarios(backend)?;

    // 4. Active situations
    let situations = build_situations(backend)?;

    // 5. Correlation breaks
    let correlation_breaks = build_correlation_breaks(backend)?;

    // 6. Movers (>2%)
    let movers = build_movers(backend)?;

    // 7. News sentiment by category
    let news_sentiment = build_news_sentiment(backend, hours)?;

    // 8. Recent alerts
    let recent_alerts = build_recent_alerts(backend, hours)?;

    Ok(MorningBrief {
        timestamp,
        regime,
        portfolio,
        scenarios,
        situations,
        correlation_breaks,
        movers,
        news_sentiment,
        recent_alerts,
    })
}

fn build_regime(backend: &BackendConnection) -> Result<Option<RegimeSummary>> {
    let current = regime_snapshots::get_current_backend(backend)?;
    Ok(current.map(|r| RegimeSummary {
        regime: r.regime,
        confidence: r.confidence,
        drivers: r.drivers,
        vix: r.vix,
        dxy: r.dxy,
        yield_10y: r.yield_10y,
        oil: r.oil,
        gold: r.gold,
        btc: r.btc,
        recorded_at: r.recorded_at,
    }))
}

fn build_portfolio(backend: &BackendConnection) -> Result<PortfolioSnapshot> {
    let cached = get_all_cached_prices_backend(backend)?;
    let symbols: Vec<String> = cached.iter().map(|q| q.symbol.clone()).collect();
    let prev_close_map: HashMap<String, Decimal> = cached
        .iter()
        .filter_map(|q| q.previous_close.map(|pc| (q.symbol.clone(), pc)))
        .collect();

    // Fallback: get yesterday's close from price history
    let today = chrono::Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let hist_1d = get_prices_at_date_backend(backend, &symbols, &yesterday_str).unwrap_or_default();

    let mut prices = Vec::new();
    for q in &cached {
        let prev = prev_close_map
            .get(&q.symbol)
            .copied()
            .or_else(|| hist_1d.get(&q.symbol).copied());
        let change_pct = prev.and_then(|p| {
            if p.is_zero() {
                None
            } else {
                let pct = (q.price - p) / p * Decimal::from(100);
                Some(format!("{:+.2}%", pct))
            }
        });
        prices.push(PriceEntry {
            symbol: q.symbol.clone(),
            price: format!("{}", q.price),
            change_pct,
        });
    }

    Ok(PortfolioSnapshot {
        total_positions: prices.len(),
        prices,
    })
}

fn build_scenarios(backend: &BackendConnection) -> Result<Vec<ScenarioSummary>> {
    let list = scenarios::list_scenarios_backend(backend, Some("active"))?;
    Ok(list
        .into_iter()
        .map(|s| ScenarioSummary {
            name: s.name,
            probability: s.probability,
            status: s.status,
            phase: s.phase,
            description: s.description,
            updated_at: s.updated_at,
        })
        .collect())
}

fn build_situations(backend: &BackendConnection) -> Result<Vec<SituationSummary>> {
    let list = scenarios::list_scenarios_by_phase_backend(backend, "active")?;
    let mut entries = Vec::new();
    for s in &list {
        let branches = scenarios::list_branches_backend(backend, s.id)?;
        let impacts = scenarios::list_impacts_backend(backend, s.id)?;
        let indicators = scenarios::list_indicators_backend(backend, s.id)?;
        let updates = scenarios::list_updates_backend(backend, s.id, None)?;
        let triggered = indicators.iter().filter(|i| i.status == "triggered").count();

        entries.push(SituationSummary {
            id: s.id,
            name: s.name.clone(),
            probability: s.probability,
            phase: s.phase.clone(),
            branch_count: branches.len(),
            impact_count: impacts.len(),
            indicator_count: indicators.len(),
            indicators_triggered: triggered,
            update_count: updates.len(),
            updated_at: s.updated_at.clone(),
        });
    }
    Ok(entries)
}

fn build_correlation_breaks(backend: &BackendConnection) -> Result<Vec<CorrelationBreakSummary>> {
    let breaks = super::correlations::compute_breaks_backend(backend, 0.3, 10)?;
    Ok(breaks
        .into_iter()
        .map(|b| CorrelationBreakSummary {
            symbol_a: b.symbol_a,
            symbol_b: b.symbol_b,
            corr_7d: b.corr_7d,
            corr_90d: b.corr_90d,
            break_delta: b.break_delta,
        })
        .collect())
}

fn build_movers(backend: &BackendConnection) -> Result<Vec<MoverSummary>> {
    let cached = get_all_cached_prices_backend(backend)?;
    let prev_close_map: HashMap<String, Decimal> = cached
        .iter()
        .filter_map(|q| q.previous_close.map(|pc| (q.symbol.clone(), pc)))
        .collect();

    // Fallback for symbols without previous_close
    let symbols: Vec<String> = cached.iter().map(|q| q.symbol.clone()).collect();
    let today = chrono::Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let hist_1d = get_prices_at_date_backend(backend, &symbols, &yesterday_str).unwrap_or_default();

    let mut movers = Vec::new();
    for q in &cached {
        let prev = prev_close_map
            .get(&q.symbol)
            .copied()
            .or_else(|| hist_1d.get(&q.symbol).copied());
        if let Some(prev_price) = prev {
            if prev_price.is_zero() {
                continue;
            }
            let pct = (q.price - prev_price) / prev_price * Decimal::from(100);
            let abs_pct = if pct < Decimal::ZERO { -pct } else { pct };
            if abs_pct >= Decimal::from(2) {
                movers.push(MoverSummary {
                    symbol: q.symbol.clone(),
                    price: format!("{}", q.price),
                    change_pct: format!("{:+.2}%", pct),
                });
            }
        }
    }

    // Sort by absolute change descending
    movers.sort_by(|a, b| {
        let abs_a = a
            .change_pct
            .trim_end_matches('%')
            .trim_start_matches('+')
            .parse::<f64>()
            .unwrap_or(0.0)
            .abs();
        let abs_b = b
            .change_pct
            .trim_end_matches('%')
            .trim_start_matches('+')
            .parse::<f64>()
            .unwrap_or(0.0)
            .abs();
        abs_b
            .partial_cmp(&abs_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Cap at 15 movers
    movers.truncate(15);

    Ok(movers)
}

fn build_news_sentiment(
    backend: &BackendConnection,
    hours: i64,
) -> Result<Vec<SentimentCategorySummary>> {
    let entries = get_latest_news_backend(backend, 100, None, None, None, Some(hours))?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let scored = super::news_sentiment::score_all(&entries);
    let aggregates = super::news_sentiment::aggregate_by_category(&scored);

    Ok(aggregates
        .into_iter()
        .map(|a| SentimentCategorySummary {
            category: a.group,
            count: a.count,
            avg_score: a.avg_score,
            label: a.label.as_str().to_string(),
            bullish: a.bullish_count,
            bearish: a.bearish_count,
            neutral: a.neutral_count,
        })
        .collect())
}

fn build_recent_alerts(
    backend: &BackendConnection,
    hours: i64,
) -> Result<Vec<AlertSummary>> {
    let alerts = db_alerts::list_alerts_recent_backend(backend, hours, None)?;
    Ok(alerts
        .into_iter()
        .map(|a| AlertSummary {
            id: a.id,
            symbol: a.symbol,
            rule_text: a.rule_text,
            status: format!("{}", a.status),
            triggered_at: a.triggered_at,
        })
        .collect())
}

fn fmt_opt_f64(v: Option<f64>) -> String {
    match v {
        Some(val) => format!("{:.2}", val),
        None => "---".to_string(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_opt_f64_some() {
        assert_eq!(fmt_opt_f64(Some(25.123)), "25.12");
    }

    #[test]
    fn test_fmt_opt_f64_none() {
        assert_eq!(fmt_opt_f64(None), "---");
    }

    #[test]
    fn test_morning_brief_structs_serialize() {
        let brief = MorningBrief {
            timestamp: "2026-03-26T10:00:00Z".to_string(),
            regime: Some(RegimeSummary {
                regime: "risk-off".to_string(),
                confidence: Some(0.75),
                drivers: Some("VIX elevated".to_string()),
                vix: Some(25.5),
                dxy: Some(104.2),
                yield_10y: Some(4.25),
                oil: Some(68.0),
                gold: Some(3025.0),
                btc: Some(87500.0),
                recorded_at: "2026-03-26T09:00:00Z".to_string(),
            }),
            portfolio: PortfolioSnapshot {
                total_positions: 2,
                prices: vec![
                    PriceEntry {
                        symbol: "BTC-USD".to_string(),
                        price: "87500.00".to_string(),
                        change_pct: Some("+2.30%".to_string()),
                    },
                    PriceEntry {
                        symbol: "GC=F".to_string(),
                        price: "3025.00".to_string(),
                        change_pct: Some("+0.45%".to_string()),
                    },
                ],
            },
            scenarios: vec![ScenarioSummary {
                name: "Fed pivot".to_string(),
                probability: 35.0,
                status: "active".to_string(),
                phase: "active".to_string(),
                description: Some("Rate cuts begin".to_string()),
                updated_at: "2026-03-25".to_string(),
            }],
            situations: vec![],
            correlation_breaks: vec![CorrelationBreakSummary {
                symbol_a: "GC=F".to_string(),
                symbol_b: "DX-Y.NYB".to_string(),
                corr_7d: Some(-0.2),
                corr_90d: Some(-0.85),
                break_delta: 0.65,
            }],
            movers: vec![MoverSummary {
                symbol: "TSLA".to_string(),
                price: "185.30".to_string(),
                change_pct: "+4.20%".to_string(),
            }],
            news_sentiment: vec![SentimentCategorySummary {
                category: "crypto".to_string(),
                count: 12,
                avg_score: 15.5,
                label: "bullish".to_string(),
                bullish: 8,
                bearish: 2,
                neutral: 2,
            }],
            recent_alerts: vec![AlertSummary {
                id: 1,
                symbol: "BTC-USD".to_string(),
                rule_text: "BTC-USD above 85000".to_string(),
                status: "triggered".to_string(),
                triggered_at: Some("2026-03-26T08:00:00Z".to_string()),
            }],
        };

        let json_str = serde_json::to_string(&brief);
        assert!(json_str.is_ok(), "MorningBrief should serialize to JSON");
        let json_val: serde_json::Value = serde_json::from_str(&json_str.unwrap()).unwrap();
        assert_eq!(json_val["regime"]["regime"], "risk-off");
        assert_eq!(json_val["portfolio"]["total_positions"], 2);
        assert_eq!(json_val["scenarios"].as_array().unwrap().len(), 1);
        assert_eq!(json_val["correlation_breaks"].as_array().unwrap().len(), 1);
        assert_eq!(json_val["movers"].as_array().unwrap().len(), 1);
        assert_eq!(json_val["news_sentiment"].as_array().unwrap().len(), 1);
        assert_eq!(json_val["recent_alerts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_empty_morning_brief_serializes() {
        let brief = MorningBrief {
            timestamp: "2026-03-26T10:00:00Z".to_string(),
            regime: None,
            portfolio: PortfolioSnapshot {
                total_positions: 0,
                prices: vec![],
            },
            scenarios: vec![],
            situations: vec![],
            correlation_breaks: vec![],
            movers: vec![],
            news_sentiment: vec![],
            recent_alerts: vec![],
        };

        let json_str = serde_json::to_string_pretty(&brief);
        assert!(json_str.is_ok());
        let json_val: serde_json::Value = serde_json::from_str(&json_str.unwrap()).unwrap();
        assert!(json_val["regime"].is_null());
        assert_eq!(json_val["portfolio"]["total_positions"], 0);
    }

    #[test]
    fn test_price_entry_change_pct_none() {
        let entry = PriceEntry {
            symbol: "AAPL".to_string(),
            price: "175.50".to_string(),
            change_pct: None,
        };
        let json_val = serde_json::to_value(&entry).unwrap();
        assert!(json_val["change_pct"].is_null());
    }

    #[test]
    fn test_mover_summary_serializes() {
        let m = MoverSummary {
            symbol: "NVDA".to_string(),
            price: "890.00".to_string(),
            change_pct: "-3.50%".to_string(),
        };
        let json_val = serde_json::to_value(&m).unwrap();
        assert_eq!(json_val["change_pct"], "-3.50%");
    }

    #[test]
    fn test_sentiment_category_serializes() {
        let s = SentimentCategorySummary {
            category: "geopolitics".to_string(),
            count: 5,
            avg_score: -8.2,
            label: "neutral".to_string(),
            bullish: 1,
            bearish: 2,
            neutral: 2,
        };
        let json_val = serde_json::to_value(&s).unwrap();
        assert_eq!(json_val["category"], "geopolitics");
        assert_eq!(json_val["count"], 5);
    }
}
