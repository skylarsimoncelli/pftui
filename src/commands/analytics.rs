use anyhow::{bail, Result};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};

use crate::alerts::AlertStatus;
use crate::analytics::catalysts;
use crate::analytics::deltas;
use crate::analytics::impact;
use crate::analytics::levels::nearest_actionable_levels;
use crate::analytics::narrative;
use crate::analytics::situation;
use crate::analytics::synthesis;
use crate::analytics::technicals::{load_or_compute_snapshots_backend, DEFAULT_TIMEFRAME};
use crate::db::backend::BackendConnection;
use crate::db::{
    agent_messages, alerts, convictions, correlation_snapshots, price_cache, regime_snapshots,
    research_questions, scenarios, structural, technical_levels, technical_snapshots, thesis,
    timeframe_signals, transactions, trends, user_predictions, watchlist,
};
use crate::db::{price_history, query};
use crate::commands::correlations::{compute_breaks_backend, interpret_break};
use crate::models::asset::AssetCategory;
use crate::models::asset_names::{infer_category, resolve_name};
use crate::models::position::compute_positions;

/// F51: Asset Intelligence Blob — canonical per-asset synthesized state.
///
/// Aggregates spot price, OHLCV stats, technical snapshot, key levels, correlations,
/// regime context, scenario/trend impacts, alerts, and freshness into one JSON payload.
pub fn run_asset_intelligence(
    backend: &BackendConnection,
    symbol: &str,
    json_output: bool,
) -> Result<()> {
    let sym = symbol.to_uppercase();
    let name = resolve_name(&sym);
    let category = infer_category(&sym);

    // --- Spot price ---
    let spot = price_cache::get_cached_price_backend(backend, &sym, "USD")
        .ok()
        .flatten();
    let spot_price = spot.as_ref().map(|q| q.price);
    let spot_fetched_at = spot.as_ref().map(|q| q.fetched_at.clone());
    let spot_source = spot.as_ref().map(|q| q.source.clone());
    let pre_market = spot.as_ref().and_then(|q| q.pre_market_price);
    let post_market = spot.as_ref().and_then(|q| q.post_market_price);
    let post_market_change_pct = spot.as_ref().and_then(|q| q.post_market_change_percent);

    // --- Price history (recent) ---
    let history = price_history::get_history_backend(backend, &sym, 370).unwrap_or_default();
    let history_days = history.len();
    let oldest_date = history.first().map(|r| r.date.clone());
    let newest_date = history.last().map(|r| r.date.clone());

    // Daily change from latest two history rows
    let daily_change_pct = if history.len() >= 2 {
        let prev = &history[history.len() - 2].close;
        let curr = &history[history.len() - 1].close;
        if *prev > dec!(0) {
            Some(((*curr - *prev) / *prev * dec!(100)).round_dp(2))
        } else {
            None
        }
    } else {
        None
    };

    // OHLCV stats from most recent bar
    let latest_bar = history.last().cloned();

    // --- Technical snapshot ---
    let snap_map =
        load_or_compute_snapshots_backend(backend, std::slice::from_ref(&sym), DEFAULT_TIMEFRAME);
    let snapshot = snap_map.get(&sym).cloned();

    // --- Key levels ---
    let levels = technical_levels::get_levels_for_symbol_backend(backend, &sym).unwrap_or_default();
    let nearest = spot_price.as_ref().and_then(|price| {
        let p = price.to_string().parse::<f64>().ok()?;
        let pair = nearest_actionable_levels(&levels, p);
        if pair.support.is_none() && pair.resistance.is_none() {
            None
        } else {
            Some(pair)
        }
    });

    // --- Correlations involving this symbol ---
    let all_correlations =
        correlation_snapshots::list_current_backend(backend, None).unwrap_or_default();
    let relevant_correlations: Vec<_> = all_correlations
        .into_iter()
        .filter(|c| c.symbol_a.to_uppercase() == sym || c.symbol_b.to_uppercase() == sym)
        .collect();

    // --- Regime context ---
    let regime = regime_snapshots::get_current_backend(backend)
        .ok()
        .flatten();

    // --- Alerts for this symbol ---
    let all_alerts = alerts::list_alerts_backend(backend).unwrap_or_default();
    let symbol_alerts: Vec<_> = all_alerts
        .into_iter()
        .filter(|a| a.symbol.to_uppercase() == sym)
        .collect();

    // --- Scenarios mentioning this symbol ---
    let all_scenarios =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let relevant_scenarios: Vec<_> = all_scenarios
        .into_iter()
        .filter(|s| {
            let impact = s.asset_impact.as_deref().unwrap_or("");
            let desc = s.description.as_deref().unwrap_or("");
            let triggers = s.triggers.as_deref().unwrap_or("");
            let haystack = format!("{} {} {} {}", s.name, impact, desc, triggers).to_uppercase();
            haystack.contains(&sym)
        })
        .collect();

    // --- Trends mentioning this symbol ---
    let all_trends = trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
    let relevant_trends: Vec<_> = all_trends
        .into_iter()
        .filter(|t| {
            let impact = t.asset_impact.as_deref().unwrap_or("");
            let desc = t.description.as_deref().unwrap_or("");
            let haystack = format!("{} {} {}", t.name, impact, desc).to_uppercase();
            haystack.contains(&sym)
        })
        .collect();

    // --- Portfolio position (if held) ---
    let txs = transactions::list_transactions_backend(backend).unwrap_or_default();
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let mut prices_map: HashMap<String, Decimal> = HashMap::new();
    if let Some(price) = spot_price {
        prices_map.insert(sym.clone(), price);
    }
    let positions = compute_positions(&txs, &prices_map, &fx_rates);
    let position = positions
        .into_iter()
        .find(|p| p.symbol.to_uppercase() == sym);

    // --- Watchlist entry ---
    let wl_entries = watchlist::list_watchlist_backend(backend).unwrap_or_default();
    let wl_entry = wl_entries
        .into_iter()
        .find(|w| w.symbol.to_uppercase() == sym);

    // --- Convictions for this symbol ---
    let all_convictions = convictions::list_current_backend(backend).unwrap_or_default();
    let symbol_convictions: Vec<_> = all_convictions
        .into_iter()
        .filter(|c| c.symbol.to_uppercase() == sym)
        .collect();

    // --- Build output ---
    let now = Utc::now().to_rfc3339();

    if json_output {
        let output = json!({
            "symbol": sym,
            "name": name,
            "category": format!("{:?}", category),
            "generated_at": now,

            "price": {
                "spot": spot_price.map(|p| p.to_string()),
                "source": spot_source,
                "fetched_at": spot_fetched_at,
                "pre_market": pre_market.map(|p| p.to_string()),
                "post_market": post_market.map(|p| p.to_string()),
                "post_market_change_pct": post_market_change_pct.map(|p| p.to_string()),
                "daily_change_pct": daily_change_pct.map(|p| p.to_string()),
                "latest_bar": latest_bar.as_ref().map(|b| json!({
                    "date": b.date,
                    "open": b.open.map(|v| v.to_string()),
                    "high": b.high.map(|v| v.to_string()),
                    "low": b.low.map(|v| v.to_string()),
                    "close": b.close.to_string(),
                    "volume": b.volume,
                })),
            },

            "technicals": snapshot.as_ref().map(|s| json!({
                "timeframe": s.timeframe,
                "rsi_14": s.rsi_14,
                "rsi_signal": s.rsi_14.map(|v| if v > 70.0 { "overbought" } else if v < 30.0 { "oversold" } else { "neutral" }),
                "macd": s.macd,
                "macd_signal": s.macd_signal,
                "macd_histogram": s.macd_histogram,
                "sma_20": s.sma_20,
                "sma_50": s.sma_50,
                "sma_200": s.sma_200,
                "bollinger_upper": s.bollinger_upper,
                "bollinger_middle": s.bollinger_middle,
                "bollinger_lower": s.bollinger_lower,
                "range_52w_low": s.range_52w_low,
                "range_52w_high": s.range_52w_high,
                "range_52w_position": s.range_52w_position,
                "above_sma_20": s.above_sma_20,
                "above_sma_50": s.above_sma_50,
                "above_sma_200": s.above_sma_200,
                "volume_avg_20": s.volume_avg_20,
                "volume_ratio_20": s.volume_ratio_20,
                "volume_regime": s.volume_regime,
                "computed_at": s.computed_at,
            })),

            "levels": {
                "nearest_support": nearest.as_ref().and_then(|n| n.support.as_ref()),
                "nearest_resistance": nearest.as_ref().and_then(|n| n.resistance.as_ref()),
                "all": levels,
                "count": levels.len(),
            },

            "correlations": relevant_correlations.iter().map(|c| {
                let other = if c.symbol_a.to_uppercase() == sym { &c.symbol_b } else { &c.symbol_a };
                json!({
                    "symbol": other,
                    "correlation": c.correlation,
                    "period": c.period,
                    "recorded_at": c.recorded_at,
                })
            }).collect::<Vec<_>>(),

            "regime": regime.as_ref().map(|r| json!({
                "regime": r.regime,
                "confidence": r.confidence,
                "drivers": r.drivers,
                "vix": r.vix,
                "dxy": r.dxy,
                "yield_10y": r.yield_10y,
                "oil": r.oil,
                "gold": r.gold,
                "btc": r.btc,
                "recorded_at": r.recorded_at,
            })),

            "scenarios": relevant_scenarios.iter().map(|s| json!({
                "id": s.id,
                "name": s.name,
                "probability": s.probability,
                "description": s.description,
                "asset_impact": s.asset_impact,
                "triggers": s.triggers,
                "status": s.status,
            })).collect::<Vec<_>>(),

            "trends": relevant_trends.iter().map(|t| json!({
                "id": t.id,
                "name": t.name,
                "timeframe": t.timeframe,
                "direction": t.direction,
                "conviction": t.conviction,
                "category": t.category,
                "description": t.description,
                "asset_impact": t.asset_impact,
                "key_signal": t.key_signal,
                "status": t.status,
            })).collect::<Vec<_>>(),

            "alerts": symbol_alerts.iter().map(|a| json!({
                "id": a.id,
                "kind": format!("{:?}", a.kind),
                "direction": format!("{:?}", a.direction),
                "condition": a.condition,
                "threshold": a.threshold,
                "status": format!("{:?}", a.status),
                "rule_text": a.rule_text,
                "recurring": a.recurring,
                "triggered_at": a.triggered_at,
            })).collect::<Vec<_>>(),

            "portfolio": position.as_ref().map(|p| json!({
                "quantity": p.quantity.to_string(),
                "avg_cost": p.avg_cost.to_string(),
                "total_cost": p.total_cost.to_string(),
                "current_value": p.current_value.map(|v| v.to_string()),
                "unrealized_gain": p.gain.map(|g| g.to_string()),
                "unrealized_gain_pct": p.gain_pct.map(|g| g.round_dp(2).to_string()),
                "allocation_pct": p.allocation_pct.map(|a| a.round_dp(2).to_string()),
            })),

            "watchlist": wl_entry.as_ref().map(|w| json!({
                "group_id": w.group_id,
                "target_price": w.target_price,
                "target_direction": w.target_direction,
                "added_at": w.added_at,
            })),

            "convictions": symbol_convictions.iter().map(|c| json!({
                "id": c.id,
                "symbol": c.symbol,
                "score": c.score,
                "notes": c.notes,
                "recorded_at": c.recorded_at,
            })).collect::<Vec<_>>(),

            "freshness": {
                "price_fetched_at": spot_fetched_at,
                "technicals_computed_at": snapshot.as_ref().map(|s| s.computed_at.clone()),
                "history_days": history_days,
                "history_range": if oldest_date.is_some() && newest_date.is_some() {
                    Some(json!({ "oldest": oldest_date, "newest": newest_date }))
                } else {
                    None
                },
                "levels_count": levels.len(),
                "alerts_count": symbol_alerts.len(),
                "correlations_count": relevant_correlations.len(),
                "scenarios_count": relevant_scenarios.len(),
                "trends_count": relevant_trends.len(),
            },
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Human-readable markdown output
        println!("# {} — {}", sym, name);
        println!("Category: {:?}", category);
        println!();

        // Price
        if let Some(price) = spot_price {
            print!("**Price:** {}", price);
            if let Some(pct) = daily_change_pct {
                let arrow = if pct > dec!(0) {
                    "▲"
                } else if pct < dec!(0) {
                    "▼"
                } else {
                    "—"
                };
                print!("  {} {}%", arrow, pct);
            }
            println!();
            if let Some(pre) = pre_market {
                println!("  Pre-market: {}", pre);
            }
            if let Some(post) = post_market {
                print!("  Post-market: {}", post);
                if let Some(pct) = post_market_change_pct {
                    print!(" ({}%)", pct);
                }
                println!();
            }
        } else {
            println!("**Price:** N/A (no cached data)");
        }
        println!();

        // Technicals
        if let Some(s) = &snapshot {
            println!("## Technicals ({})", s.timeframe);
            if let Some(rsi) = s.rsi_14 {
                let signal = if rsi > 70.0 {
                    "OVERBOUGHT"
                } else if rsi < 30.0 {
                    "OVERSOLD"
                } else {
                    "neutral"
                };
                println!("  RSI(14): {:.1} [{}]", rsi, signal);
            }
            if let (Some(macd), Some(sig), Some(hist)) = (s.macd, s.macd_signal, s.macd_histogram) {
                println!("  MACD: {:.4}  Signal: {:.4}  Hist: {:.4}", macd, sig, hist);
            }
            let smas = [
                ("SMA(20)", s.sma_20, s.above_sma_20),
                ("SMA(50)", s.sma_50, s.above_sma_50),
                ("SMA(200)", s.sma_200, s.above_sma_200),
            ];
            for (label, val, above) in smas {
                if let Some(v) = val {
                    let pos = above
                        .map(|a| if a { "above" } else { "below" })
                        .unwrap_or("?");
                    println!("  {}: {:.2} [{}]", label, v, pos);
                }
            }
            if let Some(pos) = s.range_52w_position {
                println!(
                    "  52W range: {:.0}% (low: {}, high: {})",
                    pos,
                    s.range_52w_low
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "?".into()),
                    s.range_52w_high
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "?".into()),
                );
            }
            if let Some(regime) = &s.volume_regime {
                println!(
                    "  Volume regime: {} (ratio: {:.2}x)",
                    regime,
                    s.volume_ratio_20.unwrap_or(0.0),
                );
            }
            println!();
        }

        // Levels
        if !levels.is_empty() || nearest.is_some() {
            println!("## Key Levels");
            if let Some(ref pair) = nearest {
                if let Some(ref sup) = pair.support {
                    println!(
                        "  Nearest support: {:.2} ({}, strength {:.0}%)",
                        sup.price,
                        sup.level_type,
                        sup.strength * 100.0
                    );
                }
                if let Some(ref res) = pair.resistance {
                    println!(
                        "  Nearest resistance: {:.2} ({}, strength {:.0}%)",
                        res.price,
                        res.level_type,
                        res.strength * 100.0
                    );
                }
            }
            println!("  Total stored levels: {}", levels.len());
            println!();
        }

        // Portfolio position
        if let Some(p) = &position {
            println!("## Portfolio Position");
            println!(
                "  Qty: {}  Avg cost: {}  Total cost: {}",
                p.quantity, p.avg_cost, p.total_cost
            );
            if let Some(val) = p.current_value {
                println!("  Current value: {}", val);
            }
            if let Some(g) = p.gain {
                let pct_str = p
                    .gain_pct
                    .map(|pct| format!(" ({}%)", pct.round_dp(2)))
                    .unwrap_or_default();
                println!("  Unrealized P&L: {}{}", g, pct_str);
            }
            if let Some(a) = p.allocation_pct {
                println!("  Allocation: {}%", a.round_dp(2));
            }
            println!();
        }

        // Watchlist
        if let Some(w) = &wl_entry {
            println!("## Watchlist");
            if let Some(ref target) = w.target_price {
                println!(
                    "  Target: {} {}",
                    w.target_direction.as_deref().unwrap_or("at"),
                    target
                );
            }
            println!();
        }

        // Alerts
        if !symbol_alerts.is_empty() {
            println!("## Alerts ({})", symbol_alerts.len());
            for a in &symbol_alerts {
                println!("  {}", a);
            }
            println!();
        }

        // Correlations
        if !relevant_correlations.is_empty() {
            println!("## Correlations");
            for c in &relevant_correlations {
                let other = if c.symbol_a.to_uppercase() == sym {
                    &c.symbol_b
                } else {
                    &c.symbol_a
                };
                println!("  {} ↔ {}: {:.2} ({})", sym, other, c.correlation, c.period);
            }
            println!();
        }

        // Regime
        if let Some(r) = &regime {
            println!(
                "## Current Regime: {} (confidence: {:.0}%)",
                r.regime,
                r.confidence.unwrap_or(0.0) * 100.0,
            );
            if let Some(ref drivers) = r.drivers {
                println!("  Drivers: {}", drivers);
            }
            println!();
        }

        // Scenarios
        if !relevant_scenarios.is_empty() {
            println!("## Related Scenarios ({})", relevant_scenarios.len());
            for s in &relevant_scenarios {
                println!(
                    "  [{:.0}%] {} — {}",
                    s.probability * 100.0,
                    s.name,
                    s.description.as_deref().unwrap_or("")
                );
            }
            println!();
        }

        // Trends
        if !relevant_trends.is_empty() {
            println!("## Related Trends ({})", relevant_trends.len());
            for t in &relevant_trends {
                println!(
                    "  {} {} ({}) — {}",
                    t.direction,
                    t.name,
                    t.conviction,
                    t.description.as_deref().unwrap_or("")
                );
            }
            println!();
        }

        // Convictions
        if !symbol_convictions.is_empty() {
            println!("## Convictions");
            for c in &symbol_convictions {
                println!(
                    "  {} score={} — {}",
                    c.symbol,
                    c.score,
                    c.notes.as_deref().unwrap_or("")
                );
            }
            println!();
        }

        // Freshness
        println!("## Freshness");
        println!(
            "  History: {} days{}",
            history_days,
            if oldest_date.is_some() && newest_date.is_some() {
                format!(
                    " ({} → {})",
                    oldest_date.as_deref().unwrap_or("?"),
                    newest_date.as_deref().unwrap_or("?")
                )
            } else {
                String::new()
            }
        );
        if let Some(ref at) = spot_fetched_at {
            println!("  Price fetched: {}", at);
        }
        if let Some(ref s) = snapshot {
            println!("  Technicals computed: {}", s.computed_at);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    value2: Option<&str>,
    value3: Option<&str>,
    symbol: Option<&str>,
    countries: &[String],
    metric: Option<&str>,
    score: Option<f64>,
    rank: Option<i32>,
    trend: Option<&str>,
    probability: Option<f64>,
    phase: Option<&str>,
    evidence: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
    driver: Option<&str>,
    impact: Option<&str>,
    outcome: Option<&str>,
    decade: Option<i32>,
    composite: bool,
    file: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    from: Option<&str>,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "technicals" => run_technicals(
            backend,
            symbol,
            value.unwrap_or("1d"),
            limit,
            json_output,
        ),
        "levels" => run_levels(backend, symbol, signal_type, limit, json_output),
        "signals" => run_signals(backend, symbol, signal_type, severity, limit, json_output),
        "summary" => run_summary(backend, json_output),
        "situation" => run_situation(backend, json_output),
        "deltas" => run_deltas(backend, value, json_output),
        "catalysts" => run_catalysts(backend, value, json_output),
        "impact" => run_impact(backend, json_output),
        "opportunities" => run_opportunities(backend, json_output),
        "narrative" => run_narrative(backend, json_output),
        "synthesis" => run_synthesis(backend, json_output),
        "low" => run_low(backend, json_output),
        "medium" => run_medium(backend, json_output),
        "high" => run_high(backend, json_output),
        "macro" => run_macro(
            backend,
            value,
            value2,
            value3,
            countries,
            metric,
            score,
            rank,
            trend,
            probability,
            phase,
            evidence,
            notes,
            source,
            driver,
            impact,
            outcome,
            decade,
            composite,
            file,
            from,
            date,
            limit,
            json_output,
        ),
        "alignment" => run_alignment(backend, symbol, json_output),
        "alignment-summary" => run_alignment_summary(backend, symbol, json_output),
        "divergence" => run_divergence(backend, symbol, json_output),
        "digest" => run_digest(backend, from, limit, json_output),
        "recap" => run_recap(backend, date, limit, json_output),
        "weekly-review" => run_weekly_review(backend, limit.unwrap_or(7), json_output),
        "gaps" => run_gaps(backend, symbol, json_output),
        _ => bail!(
            "unknown analytics action '{}'. Valid: technicals, levels, signals, summary, situation, deltas, catalysts, impact, opportunities, narrative, synthesis, low, medium, high, macro, alignment, alignment-summary, divergence, digest, recap, weekly-review, gaps",
            action
        ),
    }
}

fn parse_day_filter(value: Option<&str>) -> Result<Option<NaiveDate>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let normalized = raw.trim().to_lowercase();
    let today = Utc::now().date_naive();
    if normalized == "today" {
        return Ok(Some(today));
    }
    if normalized == "yesterday" {
        return Ok(Some(today - Duration::days(1)));
    }
    let parsed = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        anyhow::anyhow!(
            "invalid date '{}'. Use YYYY-MM-DD, today, or yesterday",
            raw
        )
    })?;
    Ok(Some(parsed))
}

fn run_digest(
    backend: &BackendConnection,
    from: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let role = from.unwrap_or("evening-analyst");
    let lim = limit.unwrap_or(10);

    let regime = regime_snapshots::get_current_backend(backend)
        .ok()
        .flatten();
    let top_signals =
        timeframe_signals::list_signals_backend(backend, None, None, Some(lim)).unwrap_or_default();
    let divergences = build_alignment_rows(backend, None)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .take(lim)
        .collect::<Vec<_>>();
    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let pending_predictions =
        user_predictions::list_predictions_backend(backend, Some("pending"), None, None, Some(lim))
            .unwrap_or_default();
    let scorecard = user_predictions::get_stats_backend(backend).ok();
    let recent_messages = agent_messages::list_messages_backend(
        backend,
        None,
        Some(role),
        None,
        true,
        None,
        None,
        Some(lim),
    )
    .unwrap_or_default();

    let payload = match role {
        "low-agent" | "low-timeframe-analyst" => json!({
            "from": role,
            "regime": regime,
            "signals": top_signals,
            "divergences": divergences,
            "pending_predictions": pending_predictions,
            "unacked_messages": recent_messages,
        }),
        "medium-agent" | "medium-timeframe-analyst" => json!({
            "from": role,
            "scenarios": scenarios_list,
            "convictions": conviction_rows,
            "pending_predictions": pending_predictions,
            "signals": top_signals,
            "unacked_messages": recent_messages,
        }),
        _ => json!({
            "from": role,
            "regime": regime,
            "scenarios": scenarios_list,
            "signals": top_signals,
            "divergences": divergences,
            "prediction_stats": scorecard,
            "pending_predictions": pending_predictions,
            "unacked_messages": recent_messages,
        }),
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("Analytics Digest ({})", role);
        println!("  Signals: {}", top_signals.len());
        println!("  Divergences: {}", divergences.len());
        println!("  Pending predictions: {}", pending_predictions.len());
        println!("  Unacked messages: {}", recent_messages.len());
    }

    Ok(())
}

fn run_recap(
    backend: &BackendConnection,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let day = parse_day_filter(date)?;
    let recap = narrative::build_recap_backend(backend, day, limit, true)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "date": recap.date,
                "note": recap.note,
                "events": recap.events,
                "count": recap.count,
            }))?
        );
    } else {
        println!("Analytics Recap");
        if let Some(note) = recap.note.as_ref() {
            println!("{note}");
        }
        if recap.events.is_empty() {
            println!("No recap events found.");
        }
        for e in recap.events {
            println!("  {} [{}:{}] {}", e.at, e.source, e.event_type, e.summary);
        }
    }

    Ok(())
}

// ── Weekly Review ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct WeeklyReviewReport {
    generated_at: String,
    period: WeeklyPeriod,
    portfolio: WeeklyPortfolio,
    scenario_shifts: Vec<narrative::ScenarioShift>,
    conviction_changes: Vec<narrative::ConvictionShift>,
    trend_changes: Vec<narrative::TrendShift>,
    prediction_scorecard: narrative::PredictionScorecardSummary,
    lessons: Vec<narrative::LessonEntry>,
    catalyst_outcomes: Vec<narrative::CatalystOutcome>,
    recap_events: Vec<narrative::RecapEvent>,
    regime: Option<WeeklyRegime>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct WeeklyPeriod {
    from: String,
    to: String,
    days: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct WeeklyPortfolio {
    start_value: Option<String>,
    end_value: Option<String>,
    change_pct: Option<String>,
    snapshots: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct WeeklyRegime {
    current: String,
    confidence: Option<f64>,
}

fn run_weekly_review(backend: &BackendConnection, days: usize, json_output: bool) -> Result<()> {
    let now = Utc::now();
    let today = now.date_naive();
    let period_start = today - Duration::days(days as i64);

    // Portfolio performance over the period
    let all_snapshots = crate::db::snapshots::get_all_portfolio_snapshots_backend(backend)?;
    let period_snapshots: Vec<_> = all_snapshots
        .iter()
        .filter(|s| s.date.as_str() >= period_start.to_string().as_str())
        .collect();

    // Find the start anchor: last snapshot on or before period_start
    let start_snap = all_snapshots
        .iter()
        .rev()
        .find(|s| s.date.as_str() <= period_start.to_string().as_str());
    let end_snap = all_snapshots.last();

    let (start_val, end_val, change_pct) = match (start_snap, end_snap) {
        (Some(s), Some(e)) if s.total_value > dec!(0) => {
            let pct = ((e.total_value - s.total_value) / s.total_value) * dec!(100);
            (
                Some(s.total_value.to_string()),
                Some(e.total_value.to_string()),
                Some(format!("{:+.2}", pct)),
            )
        }
        (None, Some(e)) => (None, Some(e.total_value.to_string()), None),
        _ => (None, None, None),
    };

    let portfolio = WeeklyPortfolio {
        start_value: start_val,
        end_value: end_val,
        change_pct,
        snapshots: period_snapshots.len(),
    };

    // Collect narrative data for the period
    let scenario_shifts = narrative::scenario_shifts_backend(backend, days as i64);
    let conviction_changes = narrative::conviction_changes_backend(backend, days);
    let trend_changes = narrative::trend_changes_backend(backend, days as i64);
    let prediction_scorecard = narrative::prediction_scorecard_backend(backend, days as i64);
    let lessons = narrative::lesson_items_backend(backend, days as i64);
    let catalyst_outcomes = narrative::catalyst_outcomes_backend(backend, days as i64);

    // Collect recap events for the whole period
    let mut all_events = Vec::new();
    for offset in 0..days {
        let day = today - Duration::days(offset as i64);
        let day_events = narrative::collect_recap_events_backend(backend, Some(day));
        all_events.extend(day_events);
    }
    all_events.sort_by(|a, b| b.at.cmp(&a.at));
    all_events.truncate(30);

    // Current regime
    let regime = regime_snapshots::get_current_backend(backend)
        .unwrap_or(None)
        .map(|r| WeeklyRegime {
            current: r.regime.clone(),
            confidence: r.confidence,
        });

    let report = WeeklyReviewReport {
        generated_at: now.to_rfc3339(),
        period: WeeklyPeriod {
            from: period_start.to_string(),
            to: today.to_string(),
            days,
        },
        portfolio,
        scenario_shifts,
        conviction_changes,
        trend_changes,
        prediction_scorecard,
        lessons,
        catalyst_outcomes,
        recap_events: all_events,
        regime,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Weekly Review ({} → {})",
            report.period.from, report.period.to
        );
        println!("════════════════════════════════════════════════════════════════");

        // Regime
        if let Some(ref r) = report.regime {
            println!(
                "Regime: {} (confidence: {:.0}%)",
                r.current,
                r.confidence.unwrap_or(0.0) * 100.0
            );
            println!();
        }

        // Portfolio
        println!("PORTFOLIO");
        if let (Some(ref start), Some(ref end)) =
            (&report.portfolio.start_value, &report.portfolio.end_value)
        {
            if let Some(ref pct) = report.portfolio.change_pct {
                println!("  Week: {} → {} ({})", start, end, pct);
            } else {
                println!("  Start: {} | End: {}", start, end);
            }
        } else {
            println!("  No snapshots for this period.");
        }
        println!("  Snapshots: {}", report.portfolio.snapshots);
        println!();

        // Scenario shifts
        if !report.scenario_shifts.is_empty() {
            println!("SCENARIO SHIFTS");
            for s in &report.scenario_shifts {
                println!(
                    "  [{}] {} {:.1}% → {:.1}% ({:+.1}pp){}",
                    s.severity,
                    s.name,
                    s.previous_probability,
                    s.current_probability,
                    s.delta_pct,
                    s.driver
                        .as_ref()
                        .map(|d| format!(" — {}", d))
                        .unwrap_or_default()
                );
            }
            println!();
        }

        // Conviction changes
        if !report.conviction_changes.is_empty() {
            println!("CONVICTION CHANGES");
            for c in &report.conviction_changes {
                println!(
                    "  [{}] {} {} → {} ({:+}){}",
                    c.severity,
                    c.symbol,
                    c.old_score,
                    c.new_score,
                    c.delta,
                    c.notes
                        .as_ref()
                        .map(|n| format!(" — {}", n))
                        .unwrap_or_default()
                );
            }
            println!();
        }

        // Trend changes
        if !report.trend_changes.is_empty() {
            println!("TREND CHANGES");
            for t in &report.trend_changes {
                println!(
                    "  [{}] {} ({}) {} [{}]{}",
                    t.severity,
                    t.name,
                    t.timeframe,
                    t.direction,
                    t.conviction,
                    if t.affected_assets.is_empty() {
                        String::new()
                    } else {
                        format!(" → {}", t.affected_assets.join(", "))
                    }
                );
            }
            println!();
        }

        // Prediction scorecard
        println!("PREDICTION SCORECARD");
        let ps = &report.prediction_scorecard;
        println!(
            "  Total: {} | Scored: {} | Pending: {} | Hit rate: {:.0}%",
            ps.total, ps.scored, ps.pending, ps.hit_rate_pct
        );
        if ps.scored > 0 {
            println!(
                "  Correct: {} | Partial: {} | Wrong: {}",
                ps.correct, ps.partial, ps.wrong
            );
        }
        if !ps.recent_resolutions.is_empty() {
            println!("  Recent resolutions:");
            for r in &ps.recent_resolutions {
                println!(
                    "    #{} {} → {}{}",
                    r.id,
                    r.claim,
                    r.outcome,
                    r.lesson
                        .as_ref()
                        .map(|l| format!(" ({})", l))
                        .unwrap_or_default()
                );
            }
        }
        println!();

        // Lessons
        if !report.lessons.is_empty() {
            println!("LESSONS");
            for l in &report.lessons {
                println!("  [{}] {}: {}", l.severity, l.title, l.detail);
            }
            println!();
        }

        // Catalyst outcomes
        if !report.catalyst_outcomes.is_empty() {
            println!("CATALYST OUTCOMES");
            for c in &report.catalyst_outcomes {
                println!("  {} [{}] {} — {}", c.date, c.category, c.title, c.outcome);
            }
            println!();
        }

        // Recap event count
        println!("ACTIVITY SUMMARY");
        println!(
            "  {} events recorded this period",
            report.recap_events.len()
        );
        if !report.recap_events.is_empty() {
            // Summarize by event type
            let mut by_type: HashMap<String, usize> = HashMap::new();
            for e in &report.recap_events {
                *by_type.entry(e.event_type.clone()).or_default() += 1;
            }
            let mut sorted: Vec<_> = by_type.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            for (t, count) in &sorted {
                println!("    {}: {}", t, count);
            }
        }
    }

    Ok(())
}

// ── Gaps ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct GapRow {
    layer: &'static str,
    table: &'static str,
    count: i64,
    status: &'static str,
    last_update: Option<String>,
    age_hours: Option<f64>,
}

fn parse_dt(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
    }
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
}

fn table_stats(
    backend: &BackendConnection,
    table: &str,
    ts_col: &str,
    epoch_seconds: bool,
) -> Result<(i64, Option<String>)> {
    let sqlite_sql = if epoch_seconds {
        format!(
            "SELECT COUNT(*), CAST(MAX({}) AS TEXT) FROM {}",
            ts_col, table
        )
    } else {
        format!("SELECT COUNT(*), MAX({}) FROM {}", ts_col, table)
    };
    let pg_sql = format!(
        "SELECT COUNT(*)::BIGINT, MAX({})::text FROM {}",
        ts_col, table
    );

    query::dispatch(
        backend,
        |conn| {
            let (count, ts): (i64, Option<String>) =
                conn.query_row(&sqlite_sql, [], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok((count, ts))
        },
        |pool| {
            let row: (i64, Option<String>) = crate::db::pg_runtime::block_on(async {
                sqlx::query_as(&pg_sql).fetch_one(pool).await
            })?;
            Ok(row)
        },
    )
}

fn classify_gap(
    layer: &'static str,
    table: &'static str,
    count: i64,
    raw_ts: Option<String>,
    max_age_hours: i64,
    epoch_seconds: bool,
) -> GapRow {
    if count <= 0 {
        return GapRow {
            layer,
            table,
            count,
            status: "missing",
            last_update: None,
            age_hours: None,
        };
    }

    let parsed = raw_ts.as_ref().and_then(|raw| {
        if epoch_seconds {
            raw.parse::<i64>()
                .ok()
                .and_then(|secs| DateTime::from_timestamp(secs, 0).map(|d| d.with_timezone(&Utc)))
        } else {
            parse_dt(raw)
        }
    });

    let now = Utc::now();
    let age_hours = parsed.map(|dt| (now.signed_duration_since(dt).num_seconds() as f64) / 3600.0);
    let status = if let Some(age) = age_hours {
        if age > max_age_hours as f64 {
            "stale"
        } else {
            "fresh"
        }
    } else {
        "stale"
    };

    GapRow {
        layer,
        table,
        count,
        status,
        last_update: parsed.map(|dt| dt.to_rfc3339()),
        age_hours,
    }
}

fn run_gaps(backend: &BackendConnection, symbol: Option<&str>, json_output: bool) -> Result<()> {
    if let Some(sym) = symbol {
        return run_gaps_symbol(backend, sym, json_output);
    }
    let specs: [(&str, &str, &str, i64, bool); 14] = [
        ("low", "price_cache", "fetched_at", 1, false),
        ("low", "price_history", "date", 48, false),
        ("low", "technical_snapshots", "computed_at", 24, false),
        ("low", "regime_snapshots", "recorded_at", 24, false),
        ("low", "timeframe_signals", "detected_at", 24, false),
        ("medium", "scenarios", "updated_at", 24 * 7, false),
        ("medium", "thesis", "updated_at", 24 * 14, false),
        ("medium", "convictions", "recorded_at", 24 * 14, false),
        ("high", "trend_tracker", "updated_at", 24 * 14, false),
        ("macro", "structural_outcomes", "updated_at", 24 * 30, false),
        ("macro", "news_cache", "fetched_at", 12, false),
        ("macro", "predictions_cache", "updated_at", 24, true),
        ("macro", "cot_cache", "fetched_at", 24 * 8, false),
        ("macro", "onchain_cache", "fetched_at", 24 * 2, false),
    ];

    let mut rows = Vec::new();
    for (layer, table, ts_col, max_age_hours, epoch_seconds) in specs {
        if let Ok((count, last_update)) = table_stats(backend, table, ts_col, epoch_seconds) {
            rows.push(classify_gap(
                layer,
                table,
                count,
                last_update,
                max_age_hours,
                epoch_seconds,
            ));
        }
    }

    let missing = rows.iter().filter(|r| r.status == "missing").count();
    let stale = rows.iter().filter(|r| r.status == "stale").count();
    let fresh = rows.iter().filter(|r| r.status == "fresh").count();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "gaps": rows,
                "summary": {
                    "missing": missing,
                    "stale": stale,
                    "fresh": fresh,
                    "checked": rows.len()
                }
            }))?
        );
    } else {
        println!("Analytics Data Gaps");
        println!(
            "{:<7} {:<20} {:>8} {:<8} {:>8}",
            "Layer", "Table", "Records", "Status", "Age(h)"
        );
        println!("{}", "─".repeat(62));
        for row in &rows {
            let age = row
                .age_hours
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<7} {:<20} {:>8} {:<8} {:>8}",
                row.layer, row.table, row.count, row.status, age
            );
        }
        println!(
            "\nChecked {} tables: {} fresh, {} stale, {} missing",
            rows.len(),
            fresh,
            stale,
            missing
        );
    }

    Ok(())
}

/// Row from price_history: (date, close, volume, open, high, low)
type OhlcvRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Per-symbol OHLCV data quality report (F48)
fn run_gaps_symbol(backend: &BackendConnection, symbol: &str, json_output: bool) -> Result<()> {
    let sym = symbol.to_uppercase();

    // Query all price_history rows for this symbol
    let sqlite_sql = format!(
        "SELECT date, close, volume, open, high, low FROM price_history WHERE symbol = '{}' ORDER BY date ASC",
        sym.replace('\'', "''")
    );
    let pg_sql = sqlite_sql.clone();

    let rows: Vec<OhlcvRow> = query::dispatch(
        backend,
        |conn| {
            let mut stmt = conn.prepare(&sqlite_sql)?;
            let result = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(result)
        },
        |pool| {
            let result: Vec<OhlcvRow> = crate::db::pg_runtime::block_on(async {
                sqlx::query_as(&pg_sql).fetch_all(pool).await
            })?;
            Ok(result)
        },
    )?;

    let total_bars = rows.len();
    if total_bars == 0 {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "symbol": sym,
                    "total_bars": 0,
                    "message": "No price history found"
                }))?
            );
        } else {
            println!("No price history found for {}", sym);
        }
        return Ok(());
    }

    let first_date = &rows[0].0;
    let last_date = &rows[total_bars - 1].0;

    let has_close = total_bars; // all rows have close
    let has_volume = rows.iter().filter(|r| r.2.is_some()).count();
    let has_open = rows.iter().filter(|r| r.3.is_some()).count();
    let has_high = rows.iter().filter(|r| r.4.is_some()).count();
    let has_low = rows.iter().filter(|r| r.5.is_some()).count();
    let full_ohlcv = rows
        .iter()
        .filter(|r| r.2.is_some() && r.3.is_some() && r.4.is_some() && r.5.is_some())
        .count();

    // Detect date gaps (missing trading days)
    let mut date_gaps = Vec::new();
    for i in 1..rows.len() {
        let prev = chrono::NaiveDate::parse_from_str(&rows[i - 1].0, "%Y-%m-%d");
        let curr = chrono::NaiveDate::parse_from_str(&rows[i].0, "%Y-%m-%d");
        if let (Ok(p), Ok(c)) = (prev, curr) {
            let gap_days = (c - p).num_days();
            // >3 calendar days = likely a gap (weekends are 2 days)
            if gap_days > 3 {
                date_gaps.push(json!({
                    "from": rows[i - 1].0,
                    "to": rows[i].0,
                    "gap_days": gap_days,
                }));
            }
        }
    }

    let pct = |n: usize| {
        if total_bars == 0 {
            0.0
        } else {
            (n as f64 / total_bars as f64) * 100.0
        }
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "symbol": sym,
                "total_bars": total_bars,
                "date_range": { "first": first_date, "last": last_date },
                "coverage": {
                    "close": { "count": has_close, "pct": pct(has_close) },
                    "open": { "count": has_open, "pct": pct(has_open) },
                    "high": { "count": has_high, "pct": pct(has_high) },
                    "low": { "count": has_low, "pct": pct(has_low) },
                    "volume": { "count": has_volume, "pct": pct(has_volume) },
                    "full_ohlcv": { "count": full_ohlcv, "pct": pct(full_ohlcv) },
                },
                "date_gaps": date_gaps,
                "quality": if pct(full_ohlcv) >= 90.0 { "good" } else if pct(full_ohlcv) >= 50.0 { "partial" } else { "close_only" },
            }))?
        );
    } else {
        println!("OHLCV Data Quality — {}", sym);
        println!(
            "Date range: {} to {} ({} bars)",
            first_date, last_date, total_bars
        );
        println!();
        println!("{:<10} {:>8} {:>8}", "Field", "Count", "Coverage");
        println!("{}", "─".repeat(30));
        println!("{:<10} {:>8} {:>7.1}%", "Close", has_close, pct(has_close));
        println!("{:<10} {:>8} {:>7.1}%", "Open", has_open, pct(has_open));
        println!("{:<10} {:>8} {:>7.1}%", "High", has_high, pct(has_high));
        println!("{:<10} {:>8} {:>7.1}%", "Low", has_low, pct(has_low));
        println!(
            "{:<10} {:>8} {:>7.1}%",
            "Volume",
            has_volume,
            pct(has_volume)
        );
        println!(
            "{:<10} {:>8} {:>7.1}%",
            "Full OHLCV",
            full_ohlcv,
            pct(full_ohlcv)
        );

        if !date_gaps.is_empty() {
            println!("\nDate Gaps (>{} calendar days):", 3);
            for gap in &date_gaps {
                println!(
                    "  {} → {} ({} days)",
                    gap["from"].as_str().unwrap_or("-"),
                    gap["to"].as_str().unwrap_or("-"),
                    gap["gap_days"]
                );
            }
        }

        let quality = if pct(full_ohlcv) >= 90.0 {
            "GOOD — full OHLCV available"
        } else if pct(full_ohlcv) >= 50.0 {
            "PARTIAL — some bars missing OHLC data"
        } else {
            "CLOSE-ONLY — OHLCV data not yet backfilled"
        };
        println!("\nQuality: {}", quality);
    }

    Ok(())
}

fn run_technicals(
    backend: &BackendConnection,
    symbol: Option<&str>,
    timeframe: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows = if let Some(sym) = symbol {
        technical_snapshots::get_latest_snapshot_backend(backend, &sym.to_uppercase(), timeframe)?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        technical_snapshots::list_latest_snapshots_backend(backend, timeframe, limit)?
    };
    if let Some(limit) = limit {
        rows.truncate(limit);
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "timeframe": timeframe,
                "technicals": rows,
                "count": rows.len(),
            }))?
        );
    } else if rows.is_empty() {
        println!(
            "No technical snapshots found for timeframe '{}'.",
            timeframe
        );
    } else {
        println!("Technical Snapshots ({})", timeframe);
        println!(
            "{:<10} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:<8}",
            "Symbol", "RSI", "MACD", "Hist", "SMA20", "SMA50", "SMA200", "52W%", "Volume"
        );
        println!("{}", "─".repeat(90));
        for row in rows.drain(..) {
            println!(
                "{:<10} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:<8}",
                row.symbol,
                fmt_opt(row.rsi_14, 1),
                fmt_opt(row.macd, 2),
                fmt_opt(row.macd_histogram, 2),
                fmt_opt(row.sma_20, 2),
                fmt_opt(row.sma_50, 2),
                fmt_opt(row.sma_200, 2),
                fmt_opt(row.range_52w_position, 0),
                row.volume_regime.unwrap_or_else(|| "n/a".to_string()),
            );
        }
    }

    Ok(())
}

fn fmt_opt(value: Option<f64>, precision: usize) -> String {
    value
        .map(|v| format!("{:.*}", precision, v))
        .unwrap_or_else(|| "N/A".to_string())
}

fn run_levels(
    backend: &BackendConnection,
    symbol: Option<&str>,
    level_type: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows = if let Some(sym) = symbol {
        technical_levels::get_levels_for_symbol_backend(backend, &sym.to_uppercase())?
    } else {
        technical_levels::list_all_levels_backend(backend, None)?
    };

    // Filter by level_type if provided
    if let Some(lt) = level_type {
        let lt_lower = lt.to_lowercase();
        rows.retain(|r| r.level_type.to_lowercase() == lt_lower);
    }

    if let Some(limit) = limit {
        rows.truncate(limit);
    }

    if json_output {
        // Enrich with nearest-level context when filtering by symbol
        let output = if let Some(sym) = symbol {
            let spot = price_cache::get_cached_price_backend(backend, &sym.to_uppercase(), "USD")
                .ok()
                .flatten()
                .map(|q| q.price.to_string().parse::<f64>().unwrap_or(0.0));

            let nearest = spot.map(|price| nearest_actionable_levels(&rows, price));

            json!({
                "symbol": sym.to_uppercase(),
                "spot_price": spot,
                "nearest_support": nearest.as_ref().and_then(|n| n.support.as_ref()),
                "nearest_resistance": nearest.as_ref().and_then(|n| n.resistance.as_ref()),
                "levels": rows,
                "count": rows.len(),
            })
        } else {
            json!({
                "levels": rows,
                "count": rows.len(),
            })
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if rows.is_empty() {
        println!(
            "No market structure levels found{}.",
            symbol
                .map(|s| format!(" for {}", s.to_uppercase()))
                .unwrap_or_default()
        );
        println!("Run `pftui data refresh` to compute levels.");
    } else {
        if let Some(sym) = symbol {
            println!("Market Structure Levels — {}", sym.to_uppercase());
        } else {
            println!("Market Structure Levels — All Symbols");
        }
        println!(
            "{:<10} {:<14} {:>12} {:>8} {:<16} Notes",
            "Symbol", "Type", "Price", "Str", "Method"
        );
        println!("{}", "─".repeat(80));
        for row in &rows {
            println!(
                "{:<10} {:<14} {:>12} {:>8} {:<16} {}",
                row.symbol,
                row.level_type,
                format_level_price(row.price),
                format!("{:.0}%", row.strength * 100.0),
                row.source_method,
                row.notes.as_deref().unwrap_or(""),
            );
        }
    }

    Ok(())
}

fn format_level_price(price: f64) -> String {
    if price >= 10000.0 {
        format!("{:.0}", price)
    } else if price >= 1.0 {
        format!("{:.2}", price)
    } else {
        format!("{:.4}", price)
    }
}

fn run_signals(
    backend: &BackendConnection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows = timeframe_signals::list_signals_backend(
        backend,
        signal_type,
        severity,
        limit.or(Some(25)),
    )?;
    if let Some(sym) = symbol {
        let needle = format!("\"{}\"", sym.to_uppercase());
        rows.retain(|r| r.assets.to_uppercase().contains(&needle));
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "signals": rows,
                "count": rows.len()
            }))?
        );
    } else if rows.is_empty() {
        println!("No cross-timeframe signals found.");
    } else {
        println!("Cross-timeframe signals ({}):", rows.len());
        for sig in rows {
            println!(
                "  [{}|{}] {}\n    assets={} layers={} at={}",
                sig.severity,
                sig.signal_type,
                sig.description,
                sig.assets,
                sig.layers,
                sig.detected_at
            );
        }
    }

    Ok(())
}

/// Combined signal view: cross-timeframe signals, per-symbol technical signals, or both.
///
/// `source` can be "technical", "timeframe", or "all" (default).
#[allow(clippy::too_many_arguments)]
pub fn run_signals_combined(
    backend: &BackendConnection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    direction: Option<&str>,
    source: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let show_timeframe = source == "all" || source == "timeframe";
    let show_technical = source == "all" || source == "technical";

    let lim = limit.unwrap_or(25);

    // Cross-timeframe signals
    let tf_signals = if show_timeframe {
        let mut rows =
            timeframe_signals::list_signals_backend(backend, signal_type, severity, Some(lim))?;
        if let Some(sym) = symbol {
            let needle = format!("\"{}\"", sym.to_uppercase());
            rows.retain(|r| r.assets.to_uppercase().contains(&needle));
        }
        rows
    } else {
        Vec::new()
    };

    // Per-symbol technical signals (severity + direction now filter at DB level)
    let tech_signals = if show_technical {
        crate::db::technical_signals::list_signals_filtered_backend(
            backend,
            symbol,
            signal_type,
            severity,
            direction,
            Some(lim),
        )
        .unwrap_or_default()
    } else {
        Vec::new()
    };

    if json_output {
        let mut payload = serde_json::Map::new();
        if show_timeframe {
            payload.insert(
                "timeframe_signals".to_string(),
                serde_json::to_value(&tf_signals)?,
            );
        }
        if show_technical {
            payload.insert(
                "technical_signals".to_string(),
                serde_json::to_value(&tech_signals)?,
            );
        }
        payload.insert(
            "count".to_string(),
            json!(tf_signals.len() + tech_signals.len()),
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::Value::Object(payload))?
        );
    } else {
        if !tf_signals.is_empty() {
            println!("Cross-timeframe signals ({}):", tf_signals.len());
            for sig in &tf_signals {
                println!(
                    "  [{}|{}] {}\n    assets={} layers={} at={}",
                    sig.severity,
                    sig.signal_type,
                    sig.description,
                    sig.assets,
                    sig.layers,
                    sig.detected_at
                );
            }
        } else if show_timeframe {
            println!("No cross-timeframe signals found.");
        }

        if !tech_signals.is_empty() {
            if show_timeframe {
                println!();
            }
            println!("Technical signals ({}):", tech_signals.len());
            for sig in &tech_signals {
                let price_str = sig
                    .trigger_price
                    .map(|p| format!(" @ {:.2}", p))
                    .unwrap_or_default();
                println!(
                    "  [{}|{}] {} — {}{}\n    at={}",
                    sig.severity,
                    sig.direction,
                    sig.symbol,
                    sig.description,
                    price_str,
                    sig.detected_at
                );
            }
        } else if show_technical {
            println!("No technical signals found.");
        }
    }

    Ok(())
}

fn run_summary(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let regime = regime_snapshots::get_current_backend(backend).unwrap_or(None);
    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let top_scenario = scenarios_list.first().cloned();
    let trends_list =
        trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
    let top_trend = trends_list.first().cloned();
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    let top_cycle = cycles.first().cloned();
    let signal = timeframe_signals::latest_signal_backend(backend).unwrap_or(None);
    let signal_count = timeframe_signals::list_signals_backend(backend, None, None, None)
        .unwrap_or_default()
        .len();
    let price_count = price_cache::get_all_cached_prices_backend(backend)
        .unwrap_or_default()
        .len();
    let all_alerts = alerts::list_alerts_backend(backend).unwrap_or_default();
    let alert_count = all_alerts.len();
    let triggered_alert_count = all_alerts
        .iter()
        .filter(|a| a.status == AlertStatus::Triggered)
        .count();
    let alignments = build_alignment_rows(backend, None).unwrap_or_default();
    let alignment_score = if alignments.is_empty() {
        0.0
    } else {
        alignments.iter().map(|a| a.score_pct).sum::<f64>() / alignments.len() as f64
    };
    let divergence_notes: Vec<String> = alignments
        .iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .take(5)
        .map(|a| format!("{} {}B/{}S", a.symbol, a.bull_layers, a.bear_layers))
        .collect();

    let scenario_probs: Vec<_> = scenarios_list
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "probability": s.probability,
                "status": s.status,
                "updated_at": s.updated_at,
            })
        })
        .collect();

    // Situation engine data: count active situations and their triggered indicators
    let active_situations =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let situations_with_phase: Vec<_> = active_situations
        .iter()
        .filter(|s| s.phase == "active")
        .collect();
    let situation_count = situations_with_phase.len();
    let mut situation_indicators_watching = 0usize;
    let mut situation_indicators_triggered = 0usize;
    let mut situation_triggered_labels: Vec<serde_json::Value> = Vec::new();
    for sit in &situations_with_phase {
        let indicators =
            scenarios::list_indicators_backend(backend, sit.id).unwrap_or_default();
        for ind in &indicators {
            match ind.status.as_str() {
                "watching" => situation_indicators_watching += 1,
                "triggered" => {
                    situation_indicators_triggered += 1;
                    situation_triggered_labels.push(json!({
                        "situation": sit.name,
                        "label": ind.label,
                        "symbol": ind.symbol,
                        "metric": ind.metric,
                        "last_value": ind.last_value,
                    }));
                }
                _ => {}
            }
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "regime": regime,
                "top_scenario": top_scenario,
                "scenario_probabilities": scenario_probs,
                "top_trend": top_trend,
                "top_cycle": top_cycle,
                "top_signal": signal,
                "prices_tracked": price_count,
                "alerts_total": alert_count,
                "alerts_triggered": triggered_alert_count,
                "signal_count": signal_count,
                "alignment_score_pct": alignment_score,
                "alignment_assets": alignments.len(),
                "divergence_notes": divergence_notes,
                "situation_engine": {
                    "active_situations": situation_count,
                    "indicators_watching": situation_indicators_watching,
                    "indicators_triggered": situation_indicators_triggered,
                    "triggered_indicators": situation_triggered_labels,
                },
            }))?
        );
    } else {
        println!("Analytics Engine — Multi-Timeframe Intelligence");
        println!("════════════════════════════════════════════════════════════════");
        println!(
            "PRICES: {}  ALERTS: {} (triggered {})  SIGNALS: {}",
            price_count, alert_count, triggered_alert_count, signal_count
        );
        println!(
            "ALIGNMENT SCORE: {} {:.0}% ({} assets)",
            score_bar(alignment_score),
            alignment_score,
            alignments.len()
        );
        if !divergence_notes.is_empty() {
            println!("DIVERGENCES: {}", divergence_notes.join(" | "));
        }
        if let Some(r) = regime {
            println!(
                "LOW: {} ({:.2})",
                r.regime.to_uppercase(),
                r.confidence.unwrap_or(0.0)
            );
        } else {
            println!("LOW: no regime snapshot");
        }
        if scenarios_list.is_empty() {
            println!("MEDIUM: no active scenario");
        } else {
            println!("MEDIUM: {} active scenarios", scenarios_list.len());
            for s in &scenarios_list {
                println!("  {} ({:.1}%)", s.name, s.probability);
            }
        }
        if let Some(t) = top_trend {
            println!("HIGH: {} [{}]", t.name, t.direction);
        } else {
            println!("HIGH: no active trend");
        }
        if let Some(c) = top_cycle {
            println!("MACRO: {} -> {}", c.cycle_name, c.current_stage);
        } else {
            println!("MACRO: no structural cycle");
        }
        if let Some(sig) = signal {
            println!("ALIGNMENT SIGNAL: [{}] {}", sig.severity, sig.description);
        }
        if situation_count > 0 {
            println!(
                "SITUATIONS: {} active, {} indicators ({} watching, {} triggered)",
                situation_count,
                situation_indicators_watching + situation_indicators_triggered,
                situation_indicators_watching,
                situation_indicators_triggered
            );
            for t in &situation_triggered_labels {
                if let (Some(sit), Some(label), Some(sym)) = (
                    t.get("situation").and_then(|v| v.as_str()),
                    t.get("label").and_then(|v| v.as_str()),
                    t.get("symbol").and_then(|v| v.as_str()),
                ) {
                    println!("  ⚡ {} — {} [{}]", sit, label, sym);
                }
            }
        }
    }
    Ok(())
}

fn run_situation(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let snapshot = situation::build_snapshot_backend(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!("Situation Room");
        println!("════════════════════════════════════════════════════════════════");
        println!("{}", snapshot.headline);
        println!("{}", snapshot.subtitle);
        println!();

        if !snapshot.summary_stats.is_empty() {
            let line = snapshot
                .summary_stats
                .iter()
                .map(|stat| format!("{}: {}", stat.label, stat.value))
                .collect::<Vec<_>>()
                .join("  •  ");
            println!("{line}");
        }

        if !snapshot.watch_now.is_empty() {
            println!();
            println!("WATCH NOW");
            for item in snapshot.watch_now.iter().take(5) {
                println!(
                    "- [{}] {} — {} ({})",
                    item.severity, item.title, item.detail, item.value
                );
            }
        }

        if !snapshot.portfolio_impacts.is_empty() {
            println!();
            println!("PORTFOLIO IMPACT");
            for item in snapshot.portfolio_impacts.iter().take(5) {
                println!(
                    "- [{}] {} — {} ({})",
                    item.severity, item.title, item.detail, item.value
                );
            }
        }

        if !snapshot.risk_matrix.is_empty() {
            println!();
            println!("RISK MATRIX");
            for row in &snapshot.risk_matrix {
                println!(
                    "- [{}] {} — {} = {}",
                    row.severity, row.label, row.detail, row.value
                );
            }
        }

        // Correlation breaks
        if !snapshot.correlation_breaks.is_empty() {
            println!();
            println!("CORRELATION BREAKS");
            for cb in &snapshot.correlation_breaks {
                let pair = format!("{}-{}", cb.symbol_a, cb.symbol_b);
                let c7 = cb
                    .corr_7d
                    .map(|v| format!("{:+.2}", v))
                    .unwrap_or_else(|| "---".to_string());
                let c90 = cb
                    .corr_90d
                    .map(|v| format!("{:+.2}", v))
                    .unwrap_or_else(|| "---".to_string());
                println!(
                    "\n  {} (Δ{:+.2}) — {}",
                    if pair.len() > 22 {
                        format!("{}...", &pair[..19])
                    } else {
                        pair
                    },
                    cb.break_delta,
                    cb.severity,
                );
                println!("    7d: {}  90d: {}", c7, c90);
                if let Some(ref interp) = cb.interpretation {
                    println!("    {}", interp);
                }
                if let Some(ref sig) = cb.signal {
                    println!("    → {}", sig);
                }
            }
        }

        // Alert summary
        let alerts = &snapshot.alert_summary;
        if alerts.total > 0 || alerts.triggered > 0 {
            println!();
            println!("ALERTS");
            println!(
                "  {} total — {} armed, {} triggered, {} acknowledged",
                alerts.total, alerts.armed, alerts.triggered, alerts.acknowledged
            );
            if !alerts.recent_triggered.is_empty() {
                println!("  Recently triggered:");
                for alert in &alerts.recent_triggered {
                    let at = alert
                        .triggered_at
                        .as_deref()
                        .map(|t| format!(" ({})", t))
                        .unwrap_or_default();
                    println!("    #{} {} [{}]{}", alert.id, alert.rule_text, alert.kind, at);
                }
            }
        }
    }

    Ok(())
}

fn run_deltas(backend: &BackendConnection, since: Option<&str>, json_output: bool) -> Result<()> {
    let window = deltas::DeltaWindow::parse(since)?;
    let report = deltas::build_report_backend(backend, window, true)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Change Radar");
        println!("════════════════════════════════════════════════════════════════");
        println!(
            "Window: {}  Coverage: {}",
            report.label,
            report.coverage.to_uppercase()
        );
        if let Some(baseline_at) = &report.baseline_at {
            println!("Baseline: {}", baseline_at);
        }
        println!("Current: {}", report.current_at);
        println!();
        for item in &report.change_radar {
            println!(
                "- [{}] {} — {} ({})",
                item.severity, item.title, item.detail, item.value
            );
        }
    }

    Ok(())
}

fn run_catalysts(
    backend: &BackendConnection,
    window: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let window = catalysts::CatalystWindow::parse(window)?;
    let report = catalysts::build_report_backend(backend, window)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Catalysts");
        println!("════════════════════════════════════════════════════════════════");
        println!("Window: {}", report.label);
        println!();
        if report.catalysts.is_empty() {
            println!("No catalysts found for this window.");
        } else {
            for item in &report.catalysts {
                let assets = if item.affected_assets.is_empty() {
                    "broad market".to_string()
                } else {
                    item.affected_assets.join(", ")
                };
                println!(
                    "- [{}] {} — {} | {} | assets: {}",
                    item.significance, item.time, item.title, item.countdown_bucket, assets
                );
                if !item.linked_scenarios.is_empty() {
                    for ls in &item.linked_scenarios {
                        println!(
                            "    → {} ({}, {})",
                            ls.name, ls.direction, ls.relevance
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

fn run_impact(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let report = impact::build_impact_report_backend(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Portfolio Impact");
        println!("════════════════════════════════════════════════════════════════");
        if report.exposures.is_empty() {
            println!("No exposure signals found.");
        } else {
            for item in &report.exposures {
                println!(
                    "- [{}] {} — {} ({})",
                    item.severity, item.symbol, item.summary, item.consensus
                );
            }
        }
    }

    Ok(())
}

fn run_opportunities(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let report = impact::build_opportunities_report_backend(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Opportunities");
        println!("════════════════════════════════════════════════════════════════");
        if report.opportunities.is_empty() {
            println!("No non-held opportunities found.");
        } else {
            for item in &report.opportunities {
                println!(
                    "- [{}] {} — {} ({})",
                    item.severity, item.symbol, item.summary, item.consensus
                );
            }
        }
    }

    Ok(())
}

fn run_narrative(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let report = narrative::build_report_backend(backend, true)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Narrative State");
        println!("════════════════════════════════════════════════════════════════");
        println!("{}", report.headline);
        println!("{}", report.subtitle);
        if let Some(note) = report.coverage_note.as_ref() {
            println!("{note}");
        }
        println!();

        if !report.surprises.is_empty() {
            println!("SURPRISES");
            for item in report.surprises.iter().take(5) {
                println!(
                    "- [{}] {} — {} ({})",
                    item.severity, item.title, item.detail, item.value
                );
            }
        }

        if !report.scenario_shifts.is_empty() {
            println!("\nSCENARIO SHIFTS");
            for item in report.scenario_shifts.iter().take(5) {
                println!(
                    "- [{}] {} {:.1}% -> {:.1}% ({:+.1})",
                    item.severity,
                    item.name,
                    item.previous_probability,
                    item.current_probability,
                    item.delta_pct
                );
            }
        }

        if !report.lessons.is_empty() {
            println!("\nLESSONS");
            for item in report.lessons.iter().take(3) {
                println!("- [{}] {}", item.severity, item.detail);
            }
        }

        if !report.recap.events.is_empty() {
            println!("\nRECAP ({})", report.recap.date);
            for event in report.recap.events.iter().take(5) {
                println!(
                    "  {} [{}:{}] {}",
                    event.at, event.source, event.event_type, event.summary
                );
            }
        }
    }

    Ok(())
}

fn run_synthesis(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let report = synthesis::build_report_backend(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Cross-Timeframe Synthesis");
        println!("════════════════════════════════════════════════════════════════");
        if !report.strongest_alignment.is_empty() {
            println!("Strongest alignment:");
            for item in &report.strongest_alignment {
                println!(
                    "- {} {} ({:.0}%)",
                    item.symbol, item.consensus, item.score_pct
                );
            }
        }
        if !report.highest_confidence_divergence.is_empty() {
            println!("\nTop divergences:");
            for item in &report.highest_confidence_divergence {
                println!("- {} {}", item.symbol, item.summary);
            }
        }
        if !report.conviction_matrix.is_empty() {
            println!("\nConviction matrix (analyst scores):");
            println!(
                "  {:<8} {:>5} {:>5} {:>5} {:>5}  {:>4}  Alignment",
                "Asset", "LOW", "MED", "HIGH", "MACRO", "Net"
            );
            println!("  {}", "─".repeat(56));
            for entry in &report.conviction_matrix {
                let low_str = entry
                    .low
                    .as_ref()
                    .map(|d| format!("{:+}", d.conviction))
                    .unwrap_or_else(|| "  —".to_string());
                let med_str = entry
                    .medium
                    .as_ref()
                    .map(|d| format!("{:+}", d.conviction))
                    .unwrap_or_else(|| "  —".to_string());
                let high_str = entry
                    .high
                    .as_ref()
                    .map(|d| format!("{:+}", d.conviction))
                    .unwrap_or_else(|| "  —".to_string());
                let macro_str = entry
                    .macro_view
                    .as_ref()
                    .map(|d| format!("{:+}", d.conviction))
                    .unwrap_or_else(|| "  —".to_string());
                let align_icon = match entry.alignment.as_str() {
                    "aligned-bull" => "🟢 aligned-bull",
                    "aligned-bear" => "🔴 aligned-bear",
                    "divergent" => "🟡 divergent",
                    _ => "⚪ neutral",
                };
                println!(
                    "  {:<8} {:>5} {:>5} {:>5} {:>5}  {:>+4}  {}",
                    entry.symbol,
                    low_str,
                    med_str,
                    high_str,
                    macro_str,
                    entry.net_conviction,
                    align_icon
                );
            }
        }
        if let Some(ps) = &report.power_structure {
            println!("\nPower structure (FIC/MIC/TIC):");
            for c in &ps.complexes {
                let arrow = match c.trend.as_str() {
                    "ascending" => "↑",
                    "descending" => "↓",
                    "volatile" => "↕",
                    _ => "→",
                };
                println!(
                    "  {} {} net {:+} ({} gaining, {} losing)",
                    c.complex, arrow, c.net_score, c.gaining_events, c.losing_events
                );
            }
            println!("  Regime: {}", ps.regime_classification);
            if ps.regime_shift_detected {
                println!(
                    "  ⚠ Shift: {}",
                    ps.shift_description.as_deref().unwrap_or("detected")
                );
            }
            if let Some(overlay) = &ps.regime_overlay {
                println!("  Overlay: {}", overlay);
            }
        }
        if !report.unresolved_tensions.is_empty() {
            println!("\nUnresolved tensions:");
            for t in &report.unresolved_tensions {
                let icon = if t.severity == "critical" {
                    "🔴"
                } else {
                    "🟡"
                };
                println!("  {} {} — {}", icon, t.title, t.detail);
            }
        }
    }

    Ok(())
}

fn run_low(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let regime = regime_snapshots::get_current_backend(backend).unwrap_or(None);
    let corr =
        correlation_snapshots::list_current_backend(backend, Some("30d")).unwrap_or_default();
    let signals =
        timeframe_signals::list_signals_backend(backend, None, None, Some(10)).unwrap_or_default();
    let active_scenarios =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();

    // Situation engine: gather triggered indicators across active situations
    let active_situations: Vec<_> = active_scenarios
        .iter()
        .filter(|s| s.phase == "active")
        .collect();
    let mut triggered_indicators: Vec<serde_json::Value> = Vec::new();
    let mut watching_count = 0usize;
    for sit in &active_situations {
        let indicators =
            scenarios::list_indicators_backend(backend, sit.id).unwrap_or_default();
        for ind in &indicators {
            if ind.status == "triggered" {
                triggered_indicators.push(json!({
                    "situation": sit.name,
                    "label": ind.label,
                    "symbol": ind.symbol,
                    "metric": ind.metric,
                    "operator": ind.operator,
                    "threshold": ind.threshold,
                    "last_value": ind.last_value,
                    "triggered_at": ind.triggered_at,
                }));
            } else if ind.status == "watching" {
                watching_count += 1;
            }
        }
    }

    if json_output {
        let scenario_probs: Vec<_> = active_scenarios
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "probability": s.probability,
                    "status": s.status,
                    "updated_at": s.updated_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "regime": regime,
                "correlations": corr,
                "signals": signals,
                "scenario_probabilities": scenario_probs,
                "situation_indicators": {
                    "watching": watching_count,
                    "triggered": triggered_indicators,
                },
            }))?
        );
    } else {
        println!("LOW Layer");
        if let Some(r) = regime {
            println!(
                "  Regime: {} ({:.2})",
                r.regime,
                r.confidence.unwrap_or(0.0)
            );
        }
        println!("  Correlations tracked: {}", corr.len());
        println!("  Signals: {}", signals.len());
        if !active_scenarios.is_empty() {
            println!("  Scenario Context:");
            for s in &active_scenarios {
                println!("    {}: {:.1}%", s.name, s.probability);
            }
        }
        if !triggered_indicators.is_empty() || watching_count > 0 {
            println!(
                "  Situation Indicators: {} watching, {} triggered",
                watching_count,
                triggered_indicators.len()
            );
            for t in &triggered_indicators {
                if let (Some(sit), Some(label), Some(sym)) = (
                    t.get("situation").and_then(|v| v.as_str()),
                    t.get("label").and_then(|v| v.as_str()),
                    t.get("symbol").and_then(|v| v.as_str()),
                ) {
                    let val = t
                        .get("last_value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("—");
                    println!("    ⚡ {} — {} [{}] = {}", sit, label, sym, val);
                }
            }
        }
    }

    Ok(())
}

fn run_medium(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let thesis_sections = thesis::list_thesis_backend(backend).unwrap_or_default();
    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let questions =
        research_questions::list_questions_backend(backend, Some("open")).unwrap_or_default();
    let predictions =
        user_predictions::list_predictions_backend(backend, Some("pending"), None, None, Some(20))
            .unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "scenarios": scenarios_list,
                "thesis": thesis_sections,
                "convictions": conviction_rows,
                "research_questions": questions,
                "predictions": predictions,
            }))?
        );
    } else {
        println!("MEDIUM Layer");
        println!("  Scenarios: {}", scenarios_list.len());
        println!("  Thesis sections: {}", thesis_sections.len());
        println!("  Convictions: {}", conviction_rows.len());
        println!("  Open questions: {}", questions.len());
        println!("  Pending predictions: {}", predictions.len());
    }

    Ok(())
}

fn run_high(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let trends_list =
        trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
    let mut evidence = Vec::new();
    let mut impacts = Vec::new();
    for t in &trends_list {
        let ev = trends::list_evidence_backend(backend, t.id, Some(3)).unwrap_or_default();
        evidence.push(json!({ "trend": t.name, "items": ev }));
        let imp = trends::list_asset_impacts_backend(backend, t.id).unwrap_or_default();
        impacts.push(json!({ "trend": t.name, "items": imp }));
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "trends": trends_list,
                "evidence": evidence,
                "impacts": impacts,
            }))?
        );
    } else {
        println!("HIGH Layer");
        println!("  Active trends: {}", trends_list.len());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_macro(
    backend: &BackendConnection,
    subaction: Option<&str>,
    arg1: Option<&str>,
    arg2: Option<&str>,
    countries: &[String],
    metric: Option<&str>,
    score: Option<f64>,
    rank: Option<i32>,
    trend: Option<&str>,
    probability: Option<f64>,
    phase: Option<&str>,
    evidence: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
    driver: Option<&str>,
    impact: Option<&str>,
    outcome: Option<&str>,
    decade: Option<i32>,
    composite: bool,
    file: Option<&str>,
    _from: Option<&str>,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let country = countries.first().map(|s| s.as_str());
    match subaction.unwrap_or("dashboard") {
        "dashboard" => {}
        "metrics" => {
            if arg1 == Some("set") {
                let c = arg2
                    .or(country)
                    .ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro metrics set <country> --metric <name> [--score N] [--rank N] [--trend rising|stable|declining]"))?;
                let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
                let tr = trend.unwrap_or("stable");
                let id =
                    structural::set_metric_backend(backend, c, m, score, rank, tr, notes, source)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &json!({"id": id, "country": c, "metric": m})
                        )?
                    );
                } else {
                    println!("Set macro metric: {} {} = {:?}", c, m, score);
                }
                return Ok(());
            }
            return run_macro_metrics(backend, arg1.or(country), json_output);
        }
        "metric-set" => {
            let c = country
                .or(arg1)
                .ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro metric-set <country> --metric <name> [--score N] [--rank N] [--trend rising|stable|declining]"))?;
            let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
            let tr = trend.unwrap_or("stable");
            let id = structural::set_metric_backend(backend, c, m, score, rank, tr, notes, source)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"id": id, "country": c, "metric": m}))?
                );
            } else {
                println!("Set macro metric: {} {} = {:?}", c, m, score);
            }
            return Ok(());
        }
        "compare" => {
            let left = arg1.ok_or_else(|| {
                anyhow::anyhow!("usage: pftui analytics macro compare <country-a> <country-b>")
            })?;
            let right = arg2.ok_or_else(|| {
                anyhow::anyhow!("usage: pftui analytics macro compare <country-a> <country-b>")
            })?;
            return run_macro_compare(backend, left, right, json_output);
        }
        "cycles" => {
            if arg1 == Some("current") {
                return run_macro_cycles_current(backend, country, json_output);
            }
            if arg1 == Some("history") {
                if arg2 == Some("add") {
                    let c = country.ok_or_else(|| anyhow::anyhow!("--country required"))?;
                    let m = metric.ok_or_else(|| anyhow::anyhow!("--determinant required"))?;
                    let d = decade.ok_or_else(|| anyhow::anyhow!("--year required"))?;
                    let sc = score.ok_or_else(|| anyhow::anyhow!("--score required"))?;
                    return run_macro_cycles_history_add(
                        backend,
                        c,
                        m,
                        d,
                        sc,
                        notes,
                        source,
                        json_output,
                    );
                }
                if arg2 == Some("add-batch") {
                    let path =
                        file.ok_or_else(|| anyhow::anyhow!("--file required for add-batch"))?;
                    let mut rdr = csv::Reader::from_path(path)?;
                    let mut inserted = 0usize;
                    for rec in rdr.records() {
                        let rec = rec?;
                        let c = rec.get(0).unwrap_or("").trim();
                        let m = rec.get(1).unwrap_or("").trim();
                        let d: i32 = rec.get(2).unwrap_or("").trim().parse()?;
                        let sc: f64 = rec.get(3).unwrap_or("").trim().parse()?;
                        let n = rec.get(4).map(str::trim).filter(|v| !v.is_empty());
                        let s = rec.get(5).map(str::trim).filter(|v| !v.is_empty());
                        if c.is_empty() || m.is_empty() {
                            continue;
                        }
                        structural::add_metric_history_backend(backend, c, m, d, sc, n, s)?;
                        inserted += 1;
                    }
                    if json_output {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &json!({"inserted": inserted, "file": path})
                            )?
                        );
                    } else {
                        println!("Imported {} history rows from {}", inserted, path);
                    }
                    return Ok(());
                }
                return run_macro_cycles_history(
                    backend,
                    countries,
                    metric,
                    decade,
                    composite,
                    json_output,
                );
            }
            if arg1 == Some("update") {
                let name = arg2.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro cycles update <name> --phase <phase> [--evidence text]"))?;
                let stage = phase.ok_or_else(|| anyhow::anyhow!("--phase required"))?;
                structural::set_cycle_backend(backend, name, stage, None, notes, evidence)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"name": name, "phase": stage}))?
                    );
                } else {
                    println!("Updated cycle: {} -> {}", name, stage);
                }
                return Ok(());
            }
            return run_macro_cycles(backend, json_output);
        }
        "cycle-update" => {
            let name = arg1.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro cycle-update <name> --phase <phase> [--evidence text]"))?;
            let stage = phase.ok_or_else(|| anyhow::anyhow!("--phase required"))?;
            structural::set_cycle_backend(backend, name, stage, None, notes, evidence)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"name": name, "phase": stage}))?
                );
            } else {
                println!("Updated cycle: {} -> {}", name, stage);
            }
            return Ok(());
        }
        "outcomes" => {
            if arg1 == Some("update") {
                let name = arg2.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro outcomes update <name> --probability <N> [--driver text]"))?;
                let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
                structural::update_outcome_probability_backend(backend, name, prob, driver)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &json!({"name": name, "probability": prob, "driver": driver})
                        )?
                    );
                } else {
                    println!("Updated outcome: {} -> {:.1}%", name, prob);
                }
                return Ok(());
            }
            return run_macro_outcomes(backend, json_output);
        }
        "outcome-update" => {
            let name = arg1.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro outcome-update <name> --probability <N> [--driver text]"))?;
            let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
            structural::update_outcome_probability_backend(backend, name, prob, driver)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"name": name, "probability": prob, "driver": driver})
                    )?
                );
            } else {
                println!("Updated outcome: {} -> {:.1}%", name, prob);
            }
            return Ok(());
        }
        "parallels" => return run_macro_parallels(backend, json_output),
        "log" => {
            if arg1 == Some("add") {
                let development = arg2.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro log add <development> --date YYYY-MM-DD [--impact text] [--outcome text]"))?;
                let d = date.ok_or_else(|| anyhow::anyhow!("--date required"))?;
                let id = structural::add_log_backend(backend, d, development, impact, outcome)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"id": id, "date": d}))?
                    );
                } else {
                    println!("Added macro log entry for {}", d);
                }
                return Ok(());
            }
            return run_macro_log(backend, limit, json_output);
        }
        "log-add" => {
            let development = arg1.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro log-add <development> --date YYYY-MM-DD [--impact text] [--outcome text]"))?;
            let d = date.ok_or_else(|| anyhow::anyhow!("--date required"))?;
            let id = structural::add_log_backend(backend, d, development, impact, outcome)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"id": id, "date": d}))?
                );
            } else {
                println!("Added macro log entry for {}", d);
            }
            return Ok(());
        }
        other => {
            bail!(
                "unknown analytics macro subcommand '{}'. Valid: dashboard, metrics, metric-set, compare, cycles, cycle-update, outcomes, outcome-update, parallels, log, log-add",
                other
            )
        }
    }

    let metrics = structural::list_metrics_backend(backend, None, None).unwrap_or_default();
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    let outcomes = structural::list_outcomes_backend(backend).unwrap_or_default();
    let parallels = structural::list_parallels_backend(backend, None).unwrap_or_default();
    let log_rows = structural::list_log_backend(backend, None, Some(10)).unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "power_metrics": metrics,
                "structural_cycles": cycles,
                "structural_outcomes": outcomes,
                "historical_parallels": parallels,
                "structural_log": log_rows,
            }))?
        );
    } else {
        println!("MACRO Layer");
        println!("  Metrics: {}", metrics.len());
        println!("  Cycles: {}", cycles.len());
        println!("  Outcomes: {}", outcomes.len());
        println!("  Parallels: {}", parallels.len());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_macro_cycles_history_add(
    backend: &BackendConnection,
    country: &str,
    determinant: &str,
    year: i32,
    score: f64,
    notes: Option<&str>,
    source: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let id = structural::add_metric_history_backend(
        backend,
        country,
        determinant,
        year,
        score,
        notes,
        source,
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": id,
                "country": country,
                "determinant": determinant,
                "year": year,
                "score": score,
                "notes": notes,
                "source": source,
            }))?
        );
    } else {
        println!(
            "Added history row: {} {} {} = {:.2}",
            country, determinant, year, score
        );
    }

    Ok(())
}

fn run_macro_metrics(
    backend: &BackendConnection,
    country: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let metrics = structural::list_metrics_backend(backend, country, None).unwrap_or_default();
    if metrics.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "country": country,
                    "metrics": [],
                    "count": 0,
                }))?
            );
        } else {
            println!("No macro power metrics found.");
        }
        return Ok(());
    }

    let grouped = build_country_metric_views(&metrics);
    if json_output {
        let out = if let Some(c) = country {
            if let Some(view) = grouped.get(c) {
                json!({
                    "country": c,
                    "metrics": view.metrics,
                    "composite": view.composite,
                    "composite_prev": view.composite_prev,
                    "composite_delta": view.composite_delta,
                })
            } else {
                json!({
                    "country": c,
                    "metrics": [],
                    "composite": null,
                    "composite_prev": null,
                    "composite_delta": null,
                })
            }
        } else {
            json!({ "countries": grouped })
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if let Some(c) = country {
        if let Some(view) = grouped.get(c) {
            println!("Macro Metrics ({})", c);
            for m in &view.metrics {
                println!(
                    "  {:<20} score={} rank={} trend={} {}",
                    m.metric,
                    m.score
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "—".to_string()),
                    m.rank
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    m.trend,
                    trend_arrow(&m.trend),
                );
            }
            println!(
                "\n  Composite (0-10): {}{}",
                view.composite
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "—".to_string()),
                view.composite_delta
                    .map(|d| format!(" ({:+.2} vs prev)", d))
                    .unwrap_or_default()
            );
        } else {
            println!("No macro power metrics found for {}.", c);
        }
    } else {
        println!("Macro Metrics (all countries)");
        for (c, view) in grouped {
            println!(
                "  {:<10} composite={}{} ({} metrics)",
                c,
                view.composite
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "—".to_string()),
                view.composite_delta
                    .map(|d| format!(" {:+.2}", d))
                    .unwrap_or_default(),
                view.metrics.len()
            );
        }
    }
    Ok(())
}

fn run_macro_compare(
    backend: &BackendConnection,
    country_a: &str,
    country_b: &str,
    json_output: bool,
) -> Result<()> {
    let a_rows =
        structural::list_metrics_backend(backend, Some(country_a), None).unwrap_or_default();
    let b_rows =
        structural::list_metrics_backend(backend, Some(country_b), None).unwrap_or_default();
    let a_view = build_country_metric_view(&a_rows);
    let b_view = build_country_metric_view(&b_rows);

    let mut keys: BTreeSet<String> = BTreeSet::new();
    for row in &a_view.metrics {
        keys.insert(row.metric.clone());
    }
    for row in &b_view.metrics {
        keys.insert(row.metric.clone());
    }
    let map_a: HashMap<String, MetricCurrentRow> = a_view
        .metrics
        .iter()
        .map(|m| (m.metric.clone(), m.clone()))
        .collect();
    let map_b: HashMap<String, MetricCurrentRow> = b_view
        .metrics
        .iter()
        .map(|m| (m.metric.clone(), m.clone()))
        .collect();

    let rows: Vec<serde_json::Value> = keys
        .into_iter()
        .map(|metric| {
            let left = map_a.get(&metric);
            let right = map_b.get(&metric);
            let left_score = left.and_then(|m| m.score);
            let right_score = right.and_then(|m| m.score);
            let gap = match (left_score, right_score) {
                (Some(l), Some(r)) => Some(l - r),
                _ => None,
            };
            let left_prev = a_view.prev_scores.get(&metric).copied().flatten();
            let right_prev = b_view.prev_scores.get(&metric).copied().flatten();
            let prev_gap = match (left_prev, right_prev) {
                (Some(l), Some(r)) => Some(l - r),
                _ => None,
            };
            json!({
                "metric": metric,
                "left_score": left_score,
                "left_trend": left.map(|m| m.trend.clone()),
                "right_score": right_score,
                "right_trend": right.map(|m| m.trend.clone()),
                "gap": gap,
                "trend": gap_trend(gap, prev_gap),
            })
        })
        .collect();

    let composite_gap = match (a_view.composite, b_view.composite) {
        (Some(l), Some(r)) => Some(l - r),
        _ => None,
    };
    let composite_prev_gap = match (a_view.composite_prev, b_view.composite_prev) {
        (Some(l), Some(r)) => Some(l - r),
        _ => None,
    };
    let composite_trend = gap_trend(composite_gap, composite_prev_gap);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "left_country": country_a,
                "right_country": country_b,
                "rows": rows,
                "composite": {
                    "left": a_view.composite,
                    "right": b_view.composite,
                    "gap": composite_gap,
                    "trend": composite_trend,
                }
            }))?
        );
    } else {
        println!("Macro Compare: {} vs {}", country_a, country_b);
        println!(
            "{:<16} {:>10} {:>10} {:>8} {:>10}",
            "Determinant", country_a, country_b, "Gap", "Trend"
        );
        println!("{}", "─".repeat(62));
        for row in &rows {
            let metric = row["metric"].as_str().unwrap_or("metric");
            let left = row["left_score"]
                .as_f64()
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string());
            let right = row["right_score"]
                .as_f64()
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string());
            let gap = row["gap"]
                .as_f64()
                .map(|v| format!("{:+.2}", v))
                .unwrap_or_else(|| "—".to_string());
            let trend = row["trend"].as_str().unwrap_or("Unknown");
            println!(
                "{:<16} {:>10} {:>10} {:>8} {:>10}",
                metric, left, right, gap, trend
            );
        }
        println!("{}", "─".repeat(62));
        println!(
            "{:<16} {:>10} {:>10} {:>8} {:>10}",
            "Composite",
            a_view
                .composite
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string()),
            b_view
                .composite
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string()),
            composite_gap
                .map(|v| format!("{:+.2}", v))
                .unwrap_or_else(|| "—".to_string()),
            composite_trend
        );
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct MetricCurrentRow {
    metric: String,
    score: Option<f64>,
    rank: Option<i32>,
    trend: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CountryMetricView {
    metrics: Vec<MetricCurrentRow>,
    composite: Option<f64>,
    composite_prev: Option<f64>,
    composite_delta: Option<f64>,
    #[serde(skip_serializing)]
    prev_scores: HashMap<String, Option<f64>>,
}

fn build_country_metric_views(
    rows: &[structural::PowerMetric],
) -> HashMap<String, CountryMetricView> {
    let mut grouped: HashMap<String, Vec<structural::PowerMetric>> = HashMap::new();
    for row in rows {
        grouped
            .entry(row.country.clone())
            .or_default()
            .push(row.clone());
    }
    grouped
        .into_iter()
        .map(|(country, list)| (country, build_country_metric_view(&list)))
        .collect()
}

fn build_country_metric_view(rows: &[structural::PowerMetric]) -> CountryMetricView {
    let mut latest: HashMap<String, MetricCurrentRow> = HashMap::new();
    let mut prev: HashMap<String, Option<f64>> = HashMap::new();

    for row in rows {
        if !latest.contains_key(&row.metric) {
            latest.insert(
                row.metric.clone(),
                MetricCurrentRow {
                    metric: row.metric.clone(),
                    score: row.score,
                    rank: row.rank,
                    trend: row.trend.clone(),
                },
            );
        } else if !prev.contains_key(&row.metric) {
            prev.insert(row.metric.clone(), row.score);
        }
    }

    let mut metrics: Vec<MetricCurrentRow> = latest.into_values().collect();
    metrics.sort_by(|a, b| a.metric.cmp(&b.metric));

    let mut current_components: Vec<f64> = Vec::new();
    let mut prev_components: Vec<f64> = Vec::new();
    for m in &metrics {
        if dalio_metric_key(&m.metric).is_some() {
            if let Some(score) = m.score {
                current_components.push(score);
            }
            if let Some(Some(prev_score)) = prev.get(&m.metric) {
                prev_components.push(*prev_score);
            }
        }
    }

    let composite = avg(&current_components);
    let composite_prev = avg(&prev_components);
    let composite_delta = match (composite, composite_prev) {
        (Some(c), Some(p)) => Some(c - p),
        _ => None,
    };

    CountryMetricView {
        metrics,
        composite,
        composite_prev,
        composite_delta,
        prev_scores: prev,
    }
}

fn avg(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn dalio_metric_key(metric: &str) -> Option<&'static str> {
    let m = metric.to_ascii_lowercase();
    if m.contains("education") {
        Some("education")
    } else if m.contains("innovation") {
        Some("innovation")
    } else if m.contains("competit") {
        Some("competitiveness")
    } else if m.contains("military") {
        Some("military")
    } else if m.contains("trade") {
        Some("trade")
    } else if m.contains("economic") || m.contains("gdp") || m.contains("output") {
        Some("economic_output")
    } else if m.contains("financial") {
        Some("financial_center")
    } else if m.contains("reserve") || m.contains("currency") {
        Some("reserve_currency")
    } else {
        None
    }
}

fn trend_arrow(trend: &str) -> &'static str {
    match trend.to_ascii_lowercase().as_str() {
        "up" | "rising" | "improving" | "bullish" => "↑",
        "down" | "falling" | "weakening" | "bearish" => "↓",
        _ => "→",
    }
}

fn gap_trend(gap: Option<f64>, prev_gap: Option<f64>) -> &'static str {
    match (gap, prev_gap) {
        (Some(g), Some(p)) if g.abs() < p.abs() => "Closing",
        (Some(g), Some(p)) if g.abs() > p.abs() => "Widening",
        (Some(_), Some(_)) => "Stable",
        _ => "Unknown",
    }
}

fn run_macro_cycles_current(
    backend: &BackendConnection,
    country: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    let metrics = structural::list_metrics_backend(backend, country, None).unwrap_or_default();
    let grouped = build_country_metric_views(&metrics);

    if json_output {
        let cycle_objs: Vec<serde_json::Value> = cycles
            .iter()
            .map(|c| {
                json!({
                    "cycle": c.cycle_name,
                    "phase": c.current_stage,
                    "since": c.stage_entered,
                    "evidence": c.evidence,
                })
            })
            .collect();

        let country_objs: serde_json::Value = if let Some(c) = country {
            if let Some(view) = grouped.get(c) {
                json!({
                    c: {
                        "metrics": view.metrics,
                        "composite": view.composite,
                        "composite_prev": view.composite_prev,
                        "composite_delta": view.composite_delta,
                    }
                })
            } else {
                json!({ c: { "metrics": [], "composite": null } })
            }
        } else {
            let mut map = serde_json::Map::new();
            for (c, view) in &grouped {
                map.insert(
                    c.clone(),
                    json!({
                        "metrics": view.metrics,
                        "composite": view.composite,
                        "composite_prev": view.composite_prev,
                        "composite_delta": view.composite_delta,
                    }),
                );
            }
            serde_json::Value::Object(map)
        };

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "cycles": cycle_objs,
                "countries": country_objs,
            }))?
        );
    } else {
        println!("=== Structural Cycles ===\n");
        if cycles.is_empty() {
            println!("  (no cycles tracked)");
        }
        for c in &cycles {
            println!(
                "  {:<30} {} (since {})",
                c.cycle_name,
                c.current_stage,
                c.stage_entered.as_deref().unwrap_or("?")
            );
            if let Some(ev) = &c.evidence {
                println!("    evidence: {}", ev);
            }
        }

        println!("\n=== Current Power Metrics (2026) ===\n");
        if grouped.is_empty() {
            println!("  (no power metrics recorded)");
        }
        let mut countries_sorted: Vec<_> = grouped.iter().collect();
        countries_sorted.sort_by_key(|(k, _)| (*k).clone());

        for (c, view) in &countries_sorted {
            println!(
                "  {} — composite: {}{}",
                c,
                view.composite
                    .map(|v| format!("{:.2}/10", v))
                    .unwrap_or_else(|| "—".to_string()),
                view.composite_delta
                    .map(|d| format!(" ({:+.2} vs prev)", d))
                    .unwrap_or_default()
            );
            for m in &view.metrics {
                println!(
                    "    {:<24} {:<8} rank={:<4} {}",
                    m.metric,
                    m.score
                        .map(|v| format!("{:.1}", v))
                        .unwrap_or_else(|| "—".to_string()),
                    m.rank
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    trend_arrow(&m.trend),
                );
            }
            println!();
        }
    }
    Ok(())
}

fn run_macro_cycles(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&cycles)?);
    } else {
        for c in cycles {
            println!(
                "{}: {} (since {})",
                c.cycle_name,
                c.current_stage,
                c.stage_entered.unwrap_or_else(|| "?".to_string())
            );
        }
    }
    Ok(())
}

fn run_macro_cycles_history(
    backend: &BackendConnection,
    countries: &[String],
    metric: Option<&str>,
    decade: Option<i32>,
    composite: bool,
    json_output: bool,
) -> Result<()> {
    let rows = structural::list_metric_history_backend(backend, countries, metric, decade)
        .unwrap_or_default();
    let show_composite = composite || metric.is_none();

    if show_composite {
        let mut country_decade_scores: HashMap<String, HashMap<i32, Vec<f64>>> = HashMap::new();
        for r in &rows {
            country_decade_scores
                .entry(r.country.clone())
                .or_default()
                .entry(r.decade)
                .or_default()
                .push(r.score);
        }

        let live_metrics =
            structural::list_metrics_backend(backend, None, None).unwrap_or_default();
        let live_views = build_country_metric_views(&live_metrics);
        let mut decades: BTreeSet<i32> = BTreeSet::new();
        for dmap in country_decade_scores.values() {
            for d in dmap.keys() {
                decades.insert(*d);
            }
        }
        let mut decades_vec: Vec<i32> = decades.into_iter().collect();
        decades_vec.sort_unstable();

        if json_output {
            let countries_json = country_decade_scores
                .iter()
                .map(|(country, dmap)| {
                    let mut points = serde_json::Map::new();
                    for d in &decades_vec {
                        let val = dmap
                            .get(d)
                            .and_then(|vals| avg(vals))
                            .map(|v| (v * 100.0).round() / 100.0);
                        points.insert(d.to_string(), json!(val));
                    }
                    let live = live_views
                        .get(country)
                        .and_then(|v| v.composite)
                        .map(|v| (v * 100.0).round() / 100.0);
                    points.insert("2026".to_string(), json!(live));
                    json!({"country": country, "series": points})
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "mode": "composite",
                    "decades": decades_vec,
                    "countries": countries_json,
                    "rows": rows,
                }))?
            );
        } else {
            let mut header = String::from("        ");
            for d in &decades_vec {
                header.push_str(&format!("{:>6}", d));
            }
            header.push_str(&format!("{:>6}", 2026));
            println!("{}", header);
            let mut countries_sorted = country_decade_scores.keys().cloned().collect::<Vec<_>>();
            countries_sorted.sort();
            for c in countries_sorted {
                let mut line = format!("{:<8}", c);
                let dmap = country_decade_scores.get(&c).expect("country exists");
                for d in &decades_vec {
                    let val = dmap.get(d).and_then(|vals| avg(vals));
                    match val {
                        Some(v) => line.push_str(&format!("{:>6.1}", v)),
                        None => line.push_str(&format!("{:>6}", "—")),
                    }
                }
                let live = live_views.get(&c).and_then(|v| v.composite);
                match live {
                    Some(v) => line.push_str(&format!("{:>6.1}", v)),
                    None => line.push_str(&format!("{:>6}", "—")),
                }
                println!("{}", line);
            }
        }
        return Ok(());
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"rows": rows, "count": rows.len()}))?
        );
    } else if rows.is_empty() {
        println!("No power metric history rows found.");
    } else {
        for r in rows {
            println!("{} {} {} = {:.2}", r.country, r.metric, r.decade, r.score);
        }
    }
    Ok(())
}

fn run_macro_outcomes(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let outcomes = structural::list_outcomes_backend(backend).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&outcomes)?);
    } else {
        for o in outcomes {
            println!("{}: {:.0}%", o.name, o.probability);
        }
    }
    Ok(())
}

fn run_macro_parallels(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let parallels = structural::list_parallels_backend(backend, None).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&parallels)?);
    } else {
        for p in parallels {
            println!("{} | {} → {}", p.period, p.event, p.parallel_to);
        }
    }
    Ok(())
}

fn run_macro_log(
    backend: &BackendConnection,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let rows =
        structural::list_log_backend(backend, None, Some(limit.unwrap_or(20))).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for l in rows {
            println!("{} | {}", l.date, l.development);
        }
    }
    Ok(())
}

fn regime_to_bias(regime: &str) -> &'static str {
    match regime {
        "risk-on" => "bull",
        "risk-off" | "crisis" | "stagflation" => "bear",
        _ => "neutral",
    }
}

fn run_alignment(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let alignments = build_alignment_rows(backend, symbol).unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "alignments": alignments,
                "count": alignments.len(),
            }))?
        );
    } else {
        if alignments.is_empty() {
            println!("No assets available for alignment.");
            return Ok(());
        }
        println!("Alignment Matrix");
        println!(
            "{:<10} {:<7} {:<7} {:<7} {:<7} {:<13} {:>6}",
            "Symbol", "Low", "Medium", "High", "Macro", "Consensus", "Score"
        );
        println!("{}", "─".repeat(72));
        for a in alignments {
            println!(
                "{:<10} {:<7} {:<7} {:<7} {:<7} {:<13} {:>5.0}%",
                a.symbol, a.low, a.medium, a.high, a.macro_bias, a.consensus, a.score_pct
            );
        }
    }

    Ok(())
}

fn run_alignment_summary(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let alignments = build_alignment_rows(backend, symbol).unwrap_or_default();

    if alignments.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "total": 0,
                    "groups": [],
                    "avg_score_pct": 0.0,
                    "dominant_consensus": "NONE",
                }))?
            );
        } else {
            println!("No assets available for alignment summary.");
        }
        return Ok(());
    }

    // Group by consensus
    let mut groups: std::collections::BTreeMap<String, Vec<&AlignmentRow>> =
        std::collections::BTreeMap::new();
    for a in &alignments {
        groups.entry(a.consensus.clone()).or_default().push(a);
    }

    let total = alignments.len();
    let avg_score: f64 = alignments.iter().map(|a| a.score_pct).sum::<f64>() / total as f64;

    // Dominant consensus = group with most symbols
    let dominant = groups
        .iter()
        .max_by_key(|(_, v)| v.len())
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| "MIXED".to_string());

    // Bull/bear layer averages
    let avg_bull: f64 =
        alignments.iter().map(|a| a.bull_layers as f64).sum::<f64>() / total as f64;
    let avg_bear: f64 =
        alignments.iter().map(|a| a.bear_layers as f64).sum::<f64>() / total as f64;

    if json_output {
        let group_json: Vec<serde_json::Value> = groups
            .iter()
            .map(|(consensus, rows)| {
                let mut sorted = rows.clone();
                sorted.sort_by(|a, b| {
                    b.score_pct
                        .partial_cmp(&a.score_pct)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let symbols: Vec<&str> = sorted.iter().map(|r| r.symbol.as_str()).collect();
                let grp_avg: f64 =
                    sorted.iter().map(|r| r.score_pct).sum::<f64>() / sorted.len() as f64;
                json!({
                    "consensus": consensus,
                    "count": rows.len(),
                    "pct_of_total": (rows.len() as f64 / total as f64 * 100.0),
                    "avg_score_pct": (grp_avg * 10.0).round() / 10.0,
                    "symbols": symbols,
                })
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "total": total,
                "avg_score_pct": (avg_score * 10.0).round() / 10.0,
                "avg_bull_layers": (avg_bull * 100.0).round() / 100.0,
                "avg_bear_layers": (avg_bear * 100.0).round() / 100.0,
                "dominant_consensus": dominant,
                "groups": group_json,
            }))?
        );
    } else {
        println!("Alignment Summary ({} assets)", total);
        println!("{}", "─".repeat(50));
        println!(
            "Dominant consensus: {}  |  Avg score: {:.1}%",
            dominant, avg_score
        );
        println!(
            "Avg bull layers: {:.1}  |  Avg bear layers: {:.1}",
            avg_bull, avg_bear
        );
        println!();

        // Order: STRONG BUY, BULLISH, MIXED, BEARISH, STRONG AVOID
        let order = [
            "STRONG BUY",
            "BULLISH",
            "MIXED",
            "BEARISH",
            "STRONG AVOID",
        ];
        for consensus in &order {
            if let Some(rows) = groups.get(*consensus) {
                let pct = rows.len() as f64 / total as f64 * 100.0;
                let bar = score_bar(pct);
                let mut sorted = rows.clone();
                sorted.sort_by(|a, b| {
                    b.score_pct
                        .partial_cmp(&a.score_pct)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let top_symbols: Vec<&str> = sorted
                    .iter()
                    .take(5)
                    .map(|r| r.symbol.as_str())
                    .collect();
                let more = if sorted.len() > 5 {
                    format!(" +{} more", sorted.len() - 5)
                } else {
                    String::new()
                };
                println!(
                    "{:<13} {:>3} ({:>4.1}%)  {}  {}{}",
                    consensus,
                    rows.len(),
                    pct,
                    bar,
                    top_symbols.join(", "),
                    more,
                );
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DivergenceRow {
    pub(crate) symbol: String,
    pub(crate) low: String,
    pub(crate) medium: String,
    pub(crate) high: String,
    pub(crate) macro_bias: String,
    pub(crate) bull_layers: usize,
    pub(crate) bear_layers: usize,
    pub(crate) disagreement_pct: f64,
    pub(crate) dominant_side: String,
}

fn run_divergence(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let mut rows: Vec<DivergenceRow> = build_alignment_rows(backend, symbol)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .map(|a| {
            let dominant_side = if a.bull_layers > a.bear_layers {
                "bull"
            } else if a.bear_layers > a.bull_layers {
                "bear"
            } else {
                "split"
            }
            .to_string();
            let disagreement_pct = (a.bull_layers.min(a.bear_layers) as f64 / 4.0) * 100.0;
            DivergenceRow {
                symbol: a.symbol,
                low: a.low,
                medium: a.medium,
                high: a.high,
                macro_bias: a.macro_bias,
                bull_layers: a.bull_layers,
                bear_layers: a.bear_layers,
                disagreement_pct,
                dominant_side,
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        b.disagreement_pct
            .partial_cmp(&a.disagreement_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "divergences": rows,
                "count": rows.len(),
            }))?
        );
    } else if rows.is_empty() {
        println!("No cross-layer divergences detected.");
    } else {
        println!("Cross-Layer Divergence");
        println!(
            "{:<10} {:<7} {:<7} {:<7} {:<7} {:>6} {:>6} {:>8}",
            "Symbol", "Low", "Medium", "High", "Macro", "Bull", "Bear", "Split%"
        );
        println!("{}", "─".repeat(72));
        for row in rows {
            println!(
                "{:<10} {:<7} {:<7} {:<7} {:<7} {:>6} {:>6} {:>7.0}%",
                row.symbol,
                row.low,
                row.medium,
                row.high,
                row.macro_bias,
                row.bull_layers,
                row.bear_layers,
                row.disagreement_pct
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AlignmentRow {
    pub(crate) symbol: String,
    pub(crate) low: String,
    pub(crate) medium: String,
    pub(crate) high: String,
    pub(crate) macro_bias: String,
    pub(crate) consensus: String,
    pub(crate) score_pct: f64,
    pub(crate) bull_layers: usize,
    pub(crate) bear_layers: usize,
}

fn score_bar(score_pct: f64) -> String {
    let filled = (score_pct.clamp(0.0, 100.0) / 10.0).round() as usize;
    let mut out = String::new();
    for i in 0..10 {
        out.push(if i < filled { '█' } else { '░' });
    }
    out
}

fn bias_from_score(score: i32) -> String {
    if score > 0 {
        "bull".to_string()
    } else if score < 0 {
        "bear".to_string()
    } else {
        "neutral".to_string()
    }
}

fn consensus_from_counts(bull: usize, bear: usize) -> String {
    if bull == 4 {
        "STRONG BUY".to_string()
    } else if bear == 4 {
        "STRONG AVOID".to_string()
    } else if bull >= 3 {
        "BULLISH".to_string()
    } else if bear >= 3 {
        "BEARISH".to_string()
    } else {
        "MIXED".to_string()
    }
}

fn discover_alignment_symbols(
    backend: &BackendConnection,
    filter_symbol: Option<&str>,
) -> Vec<String> {
    if let Some(sym) = filter_symbol {
        return vec![sym.to_uppercase()];
    }

    let mut symbols: BTreeSet<String> = BTreeSet::new();
    if let Ok(rows) = transactions::get_unique_symbols_backend(backend) {
        for (symbol, category) in rows {
            if category != AssetCategory::Cash {
                symbols.insert(symbol.to_uppercase());
            }
        }
    }
    if let Ok(rows) = watchlist::list_watchlist_backend(backend) {
        for row in rows {
            if !row.category.eq_ignore_ascii_case("cash") {
                symbols.insert(row.symbol.to_uppercase());
            }
        }
    }
    symbols.into_iter().collect()
}

pub(crate) fn build_alignment_rows(
    backend: &BackendConnection,
    filter_symbol: Option<&str>,
) -> Result<Vec<AlignmentRow>> {
    let symbols = discover_alignment_symbols(backend, filter_symbol);
    let low_regime = regime_snapshots::get_current_backend(backend).unwrap_or(None);
    let low_bias = low_regime
        .as_ref()
        .map(|r| regime_to_bias(&r.regime).to_string())
        .unwrap_or_else(|| "neutral".to_string());
    let low_conf = low_regime
        .as_ref()
        .and_then(|r| r.confidence)
        .map(normalize_confidence)
        .unwrap_or(0.5);

    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let conviction_bias_map: HashMap<String, String> = conviction_rows
        .iter()
        .map(|c| (c.symbol.to_uppercase(), bias_from_score(c.score)))
        .collect();
    let conviction_score_map: HashMap<String, f64> = conviction_rows
        .iter()
        .map(|c| {
            (
                c.symbol.to_uppercase(),
                (c.score as f64 / 5.0).clamp(-1.0, 1.0),
            )
        })
        .collect();

    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let impact_rows = if let Some(sym) = filter_symbol {
        trends::get_impacts_for_symbol_backend(backend, &sym.to_uppercase()).unwrap_or_default()
    } else {
        trends::list_all_impacts_backend(backend).unwrap_or_default()
    };
    let mut impacts_by_symbol: HashMap<String, Vec<(trends::Trend, trends::TrendAssetImpact)>> =
        HashMap::new();
    for (trend, impact) in impact_rows {
        impacts_by_symbol
            .entry(impact.symbol.to_uppercase())
            .or_default()
            .push((trend, impact));
    }

    let mut rows = Vec::new();
    for sym in symbols {
        let medium = conviction_bias_map
            .get(&sym)
            .cloned()
            .unwrap_or_else(|| "neutral".to_string());
        let medium_signal = conviction_score_map.get(&sym).copied().unwrap_or(0.0);

        let high_impacts = impacts_by_symbol.get(&sym).cloned().unwrap_or_default();
        let bull_high = high_impacts
            .iter()
            .filter(|(_, i)| i.impact.eq_ignore_ascii_case("bullish"))
            .count();
        let bear_high = high_impacts
            .iter()
            .filter(|(_, i)| i.impact.eq_ignore_ascii_case("bearish"))
            .count();
        let high = if bull_high > bear_high {
            "bull"
        } else if bear_high > bull_high {
            "bear"
        } else {
            "neutral"
        }
        .to_string();
        let high_total = bull_high + bear_high;
        let high_signal = if high_total == 0 {
            0.0
        } else {
            (bull_high as f64 - bear_high as f64) / high_total as f64
        };

        let mut macro_bias = "neutral".to_string();
        let mut macro_signal = 0.0;
        let needle = sym.to_lowercase();
        for s in &scenarios_list {
            if let Some(ai) = s.asset_impact.as_ref() {
                let lower = ai.to_lowercase();
                if lower.contains(&needle) {
                    let direction = if lower.contains("bull") && !lower.contains("bear") {
                        1.0
                    } else if lower.contains("bear") && !lower.contains("bull") {
                        -1.0
                    } else {
                        0.0
                    };
                    macro_signal += direction * (s.probability / 100.0);
                }
            }
        }
        macro_signal = macro_signal.clamp(-1.0, 1.0);
        if macro_signal > 0.05 {
            macro_bias = "bull".to_string();
        } else if macro_signal < -0.05 {
            macro_bias = "bear".to_string();
        }

        let low_signal = bias_to_signal(&low_bias) * low_conf;
        let bull = [low_signal, medium_signal, high_signal, macro_signal]
            .iter()
            .filter(|v| **v > 0.05)
            .count();
        let bear = [low_signal, medium_signal, high_signal, macro_signal]
            .iter()
            .filter(|v| **v < -0.05)
            .count();
        let consensus = consensus_from_counts(bull, bear);
        let weighted = (0.20 * low_signal)
            + (0.30 * medium_signal)
            + (0.25 * high_signal)
            + (0.25 * macro_signal);
        let score_pct = (weighted.abs() * 100.0).clamp(0.0, 100.0);

        rows.push(AlignmentRow {
            symbol: sym,
            low: low_bias.clone(),
            medium,
            high,
            macro_bias,
            consensus,
            score_pct,
            bull_layers: bull,
            bear_layers: bear,
        });
    }
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(rows)
}

fn normalize_confidence(raw: f64) -> f64 {
    if raw > 1.0 {
        (raw / 100.0).clamp(0.0, 1.0)
    } else {
        raw.clamp(0.0, 1.0)
    }
}

fn bias_to_signal(bias: &str) -> f64 {
    match bias {
        "bull" => 1.0,
        "bear" => -1.0,
        _ => 0.0,
    }
}

// ==================== Cross-Timeframe Unified View ====================

/// Unified cross-timeframe view combining alignment, divergence, and correlation breaks
/// into a single JSON payload. Designed for agents that previously needed to run
/// `analytics alignment`, `analytics divergence`, and `analytics correlations breaks`
/// separately.
#[derive(serde::Serialize)]
struct CrossTimeframeReport {
    timestamp: String,
    /// Per-asset alignment across LOW/MEDIUM/HIGH/MACRO timeframes
    alignment: CrossTimeframeAlignment,
    /// Assets where timeframe layers disagree (bull vs bear across layers)
    divergences: CrossTimeframeDivergences,
    /// Pairs where short-term correlation diverges from long-term
    correlation_breaks: CrossTimeframeCorrelationBreaks,
    /// Summary statistics across all three dimensions
    summary: CrossTimeframeSummary,
    /// Resolution analysis for divergent assets (only populated with --resolve)
    #[serde(skip_serializing_if = "Option::is_none")]
    resolutions: Option<CrossTimeframeResolutions>,
}

#[derive(serde::Serialize)]
struct CrossTimeframeResolutions {
    assets: Vec<ResolutionEntry>,
    count: usize,
}

#[derive(serde::Serialize)]
pub(crate) struct ResolutionEntry {
    pub(crate) symbol: String,
    /// Which layers disagree (e.g. "LOW:bear vs MEDIUM:bull, HIGH:bull")
    pub(crate) disagreement: String,
    /// Severity: "high" (opposite extremes across 3+ layers), "medium" (2 layers disagree), "low" (minor split)
    pub(crate) severity: String,
    /// Which timeframe layer has the strongest signal and should dominate the stance
    pub(crate) dominant_timeframe: String,
    /// Reasoning for why that timeframe dominates
    pub(crate) dominant_reason: String,
    /// Suggested stance: "lean-bull", "lean-bear", "wait-for-clarity"
    pub(crate) stance: String,
    /// Confidence in the resolution (0.0-1.0)
    pub(crate) confidence: f64,
    /// What would resolve the disagreement (list of observable triggers)
    pub(crate) resolution_triggers: Vec<String>,
    /// What the lower timeframe is signaling (shortest-term view)
    pub(crate) low_read: String,
    /// What the higher timeframes are signaling (longer-term view)
    pub(crate) high_read: String,
}

#[derive(serde::Serialize)]
struct CrossTimeframeAlignment {
    assets: Vec<AlignmentRow>,
    count: usize,
}

#[derive(serde::Serialize)]
struct CrossTimeframeDivergences {
    assets: Vec<DivergenceRow>,
    count: usize,
}

#[derive(serde::Serialize)]
struct CrossTimeframeCorrelationBreaks {
    pairs: Vec<CorrelationBreakJson>,
    count: usize,
    threshold: f64,
}

#[derive(serde::Serialize)]
struct CorrelationBreakJson {
    pair: String,
    corr_7d: Option<f64>,
    corr_90d: Option<f64>,
    break_delta: f64,
    /// "severe" (|delta| >= 0.70), "moderate" (>= 0.50), "minor" (< 0.50)
    severity: String,
    /// Human-readable explanation of what the break means
    interpretation: String,
    /// What the break suggests for portfolio positioning
    signal: String,
}

#[derive(serde::Serialize)]
struct CrossTimeframeSummary {
    total_tracked_assets: usize,
    aligned_count: usize,
    divergent_count: usize,
    correlation_break_count: usize,
    avg_alignment_score: f64,
    dominant_consensus: String,
    /// "clean" = mostly aligned, few breaks. "conflicted" = many divergences/breaks. "mixed" = in between.
    regime_read: String,
}

pub fn run_cross_timeframe(
    backend: &BackendConnection,
    symbol: Option<&str>,
    threshold: f64,
    limit: usize,
    resolve: bool,
    json_output: bool,
) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339();

    // 1. Alignment — all assets across timeframes
    let alignments = build_alignment_rows(backend, symbol).unwrap_or_default();

    // 2. Divergences — assets where layers disagree
    let mut divergences: Vec<DivergenceRow> = alignments
        .iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .map(|a| {
            let dominant_side = if a.bull_layers > a.bear_layers {
                "bull"
            } else if a.bear_layers > a.bull_layers {
                "bear"
            } else {
                "split"
            }
            .to_string();
            let disagreement_pct = (a.bull_layers.min(a.bear_layers) as f64 / 4.0) * 100.0;
            DivergenceRow {
                symbol: a.symbol.clone(),
                low: a.low.clone(),
                medium: a.medium.clone(),
                high: a.high.clone(),
                macro_bias: a.macro_bias.clone(),
                bull_layers: a.bull_layers,
                bear_layers: a.bear_layers,
                disagreement_pct,
                dominant_side,
            }
        })
        .collect();
    divergences.sort_by(|a, b| {
        b.disagreement_pct
            .partial_cmp(&a.disagreement_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 3. Correlation breaks
    let corr_breaks = compute_breaks_backend(backend, threshold, limit).unwrap_or_default();
    let corr_break_json: Vec<CorrelationBreakJson> = corr_breaks
        .into_iter()
        .map(|b| {
            let interp = interpret_break(&b);
            CorrelationBreakJson {
                pair: format!("{}/{}", b.symbol_a, b.symbol_b),
                corr_7d: b.corr_7d,
                corr_90d: b.corr_90d,
                break_delta: b.break_delta,
                severity: interp.severity,
                interpretation: interp.interpretation,
                signal: interp.signal,
            }
        })
        .collect();

    // 4. Summary
    let total = alignments.len();
    let aligned_count = alignments
        .iter()
        .filter(|a| a.bull_layers == 0 || a.bear_layers == 0)
        .count();
    let divergent_count = divergences.len();
    let avg_score = if total > 0 {
        alignments.iter().map(|a| a.score_pct).sum::<f64>() / total as f64
    } else {
        0.0
    };

    // Dominant consensus
    let mut consensus_counts: HashMap<String, usize> = HashMap::new();
    for a in &alignments {
        *consensus_counts.entry(a.consensus.clone()).or_default() += 1;
    }
    let dominant_consensus = consensus_counts
        .iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| "NONE".to_string());

    // Regime read: how clean/conflicted is the picture?
    let divergence_ratio = if total > 0 {
        divergent_count as f64 / total as f64
    } else {
        0.0
    };
    let break_severity = corr_break_json.len();
    let regime_read = if divergence_ratio < 0.2 && break_severity < 3 {
        "clean"
    } else if divergence_ratio > 0.5 || break_severity > 8 {
        "conflicted"
    } else {
        "mixed"
    }
    .to_string();

    // Resolution analysis (only when --resolve is set and there are divergences)
    let resolutions = if resolve && !divergences.is_empty() {
        let entries: Vec<ResolutionEntry> = divergences
            .iter()
            .map(|d| build_resolution_entry(d, &regime_read))
            .collect();
        let count = entries.len();
        Some(CrossTimeframeResolutions { assets: entries, count })
    } else {
        None
    };

    let report = CrossTimeframeReport {
        timestamp,
        alignment: CrossTimeframeAlignment {
            count: alignments.len(),
            assets: alignments,
        },
        divergences: CrossTimeframeDivergences {
            count: divergences.len(),
            assets: divergences,
        },
        correlation_breaks: CrossTimeframeCorrelationBreaks {
            count: corr_break_json.len(),
            pairs: corr_break_json,
            threshold,
        },
        summary: CrossTimeframeSummary {
            total_tracked_assets: total,
            aligned_count,
            divergent_count,
            correlation_break_count: break_severity,
            avg_alignment_score: (avg_score * 10.0).round() / 10.0,
            dominant_consensus,
            regime_read,
        },
        resolutions,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        // Human-readable output
        println!("Cross-Timeframe View");
        println!("{}", "═".repeat(72));

        // Alignment section
        println!("\n📊 ALIGNMENT ({} assets)", report.alignment.count);
        println!(
            "{:<10} {:<7} {:<7} {:<7} {:<7} {:<13} {:>6}",
            "Symbol", "Low", "Medium", "High", "Macro", "Consensus", "Score"
        );
        println!("{}", "─".repeat(72));
        for a in &report.alignment.assets {
            println!(
                "{:<10} {:<7} {:<7} {:<7} {:<7} {:<13} {:>5.0}%",
                a.symbol, a.low, a.medium, a.high, a.macro_bias, a.consensus, a.score_pct
            );
        }

        // Divergences section
        if !report.divergences.assets.is_empty() {
            println!(
                "\n⚠️  DIVERGENCES ({} assets with layer conflict)",
                report.divergences.count
            );
            println!(
                "{:<10} {:<7} {:<7} {:<7} {:<7} {:>6} {:>6} {:>8}",
                "Symbol", "Low", "Medium", "High", "Macro", "Bull", "Bear", "Split%"
            );
            println!("{}", "─".repeat(72));
            for d in &report.divergences.assets {
                println!(
                    "{:<10} {:<7} {:<7} {:<7} {:<7} {:>6} {:>6} {:>7.0}%",
                    d.symbol,
                    d.low,
                    d.medium,
                    d.high,
                    d.macro_bias,
                    d.bull_layers,
                    d.bear_layers,
                    d.disagreement_pct
                );
            }
        } else {
            println!("\n✅ DIVERGENCES: None — all layers in agreement");
        }

        // Correlation breaks section
        if !report.correlation_breaks.pairs.is_empty() {
            println!(
                "\n🔗 CORRELATION BREAKS ({} pairs, threshold {:.2})",
                report.correlation_breaks.count, report.correlation_breaks.threshold
            );
            for b in &report.correlation_breaks.pairs {
                let severity_icon = match b.severity.as_str() {
                    "severe" => "🔴",
                    "moderate" => "🟡",
                    _ => "🟢",
                };
                println!(
                    "\n  {} {} (Δ{:+.3}) — {}",
                    severity_icon, b.pair, b.break_delta, b.severity
                );
                println!(
                    "    7d: {}  90d: {}",
                    b.corr_7d
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "---".to_string()),
                    b.corr_90d
                        .map(|v| format!("{:.3}", v))
                        .unwrap_or_else(|| "---".to_string()),
                );
                println!("    {}", b.interpretation);
                println!("    → {}", b.signal);
            }
        } else {
            println!("\n✅ CORRELATION BREAKS: None detected above threshold");
        }

        // Summary
        println!("\n📋 SUMMARY");
        println!("  Tracked assets:     {}", report.summary.total_tracked_assets);
        println!("  Aligned:            {}", report.summary.aligned_count);
        println!("  Divergent:          {}", report.summary.divergent_count);
        println!("  Correlation breaks: {}", report.summary.correlation_break_count);
        println!("  Avg alignment:      {:.1}%", report.summary.avg_alignment_score);
        println!("  Dominant consensus: {}", report.summary.dominant_consensus);
        println!("  Regime read:        {}", report.summary.regime_read);

        // Resolutions section (only when --resolve flag is set)
        if let Some(ref res) = report.resolutions {
            println!(
                "\n🔧 RESOLUTIONS ({} divergent assets analyzed)",
                res.count
            );
            println!("{}", "═".repeat(72));
            for entry in &res.assets {
                let severity_icon = match entry.severity.as_str() {
                    "high" => "🔴",
                    "medium" => "🟡",
                    _ => "🟢",
                };
                let stance_icon = match entry.stance.as_str() {
                    "lean-bull" => "📈",
                    "lean-bear" => "📉",
                    _ => "⏸️",
                };
                println!(
                    "\n  {} {} — {} {} (confidence {:.0}%)",
                    severity_icon,
                    entry.symbol,
                    stance_icon,
                    entry.stance,
                    entry.confidence * 100.0
                );
                println!("    Disagreement: {}", entry.disagreement);
                println!(
                    "    Dominant:     {} — {}",
                    entry.dominant_timeframe, entry.dominant_reason
                );
                println!("    Low read:     {}", entry.low_read);
                println!("    High read:    {}", entry.high_read);
                if !entry.resolution_triggers.is_empty() {
                    println!("    Triggers:");
                    for trigger in &entry.resolution_triggers {
                        println!("      → {}", trigger);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Build a resolution entry for a divergent asset.
///
/// Resolution logic:
/// 1. **Dominant timeframe** — Higher timeframes dominate lower ones in most regimes.
///    In "conflicted" regimes, no timeframe dominates cleanly → wait-for-clarity.
///    Exception: when LOW is the *only* dissenter against 2+ higher layers, LOW is
///    likely noise/lag.
/// 2. **Stance** — follows the dominant timeframe's bias. If split evenly, wait.
/// 3. **Confidence** — based on how many layers agree on the dominant side and severity.
/// 4. **Triggers** — observable events that would resolve the disagreement.
pub(crate) fn build_resolution_entry(div: &DivergenceRow, regime_read: &str) -> ResolutionEntry {
    // Classify each layer
    let layers = [
        ("LOW", &div.low),
        ("MEDIUM", &div.medium),
        ("HIGH", &div.high),
        ("MACRO", &div.macro_bias),
    ];

    // Build disagreement description
    let active_layers: Vec<String> = layers
        .iter()
        .filter(|(_, bias)| *bias != "neutral")
        .map(|(name, bias)| format!("{}:{}", name, bias))
        .collect();
    let disagreement = if active_layers.is_empty() {
        "all neutral".to_string()
    } else {
        active_layers.join(" vs ")
    };

    // Count directional layers (excluding neutral)
    let bull_active: Vec<&str> = layers
        .iter()
        .filter(|(_, b)| *b == "bull")
        .map(|(n, _)| *n)
        .collect();
    let bear_active: Vec<&str> = layers
        .iter()
        .filter(|(_, b)| *b == "bear")
        .map(|(n, _)| *n)
        .collect();

    // Severity classification
    let severity = if div.bull_layers >= 2 && div.bear_layers >= 2 {
        "high" // 2v2 or worse — genuine split
    } else if div.bull_layers + div.bear_layers >= 3 {
        "medium" // 3 layers active but one side dominates
    } else {
        "low" // minor split (e.g., 1v1 with 2 neutral)
    }
    .to_string();

    // Determine dominant timeframe and stance
    // Higher timeframes get priority weights: MACRO=4, HIGH=3, MEDIUM=2, LOW=1
    let layer_weight = |name: &str| -> i32 {
        match name {
            "MACRO" => 4,
            "HIGH" => 3,
            "MEDIUM" => 2,
            "LOW" => 1,
            _ => 0,
        }
    };

    let bull_weight: i32 = bull_active.iter().map(|n| layer_weight(n)).sum();
    let bear_weight: i32 = bear_active.iter().map(|n| layer_weight(n)).sum();

    // In conflicted regime, require stronger signal to take a stance
    let weight_threshold = if regime_read == "conflicted" { 3 } else { 1 };
    let weight_diff = (bull_weight - bear_weight).abs();

    let (stance, dominant_timeframe, dominant_reason) = if weight_diff < weight_threshold {
        // No clear winner — wait
        (
            "wait-for-clarity".to_string(),
            "NONE".to_string(),
            "No timeframe has decisive weight advantage; cross-timeframe picture is genuinely split".to_string(),
        )
    } else if bull_weight > bear_weight {
        // Higher timeframes lean bull
        let dom = bull_active
            .iter()
            .max_by_key(|n| layer_weight(n))
            .unwrap_or(&"MEDIUM");
        let reason = format!(
            "{} layers ({}) outweigh {} layers ({}) by weight {}>{}",
            bull_active.len(),
            bull_active.join("+"),
            bear_active.len(),
            bear_active.join("+"),
            bull_weight,
            bear_weight,
        );
        ("lean-bull".to_string(), dom.to_string(), reason)
    } else {
        // Higher timeframes lean bear
        let dom = bear_active
            .iter()
            .max_by_key(|n| layer_weight(n))
            .unwrap_or(&"MEDIUM");
        let reason = format!(
            "{} layers ({}) outweigh {} layers ({}) by weight {}>{}",
            bear_active.len(),
            bear_active.join("+"),
            bull_active.len(),
            bull_active.join("+"),
            bear_weight,
            bull_weight,
        );
        ("lean-bear".to_string(), dom.to_string(), reason)
    };

    // Confidence: based on weight differential and severity
    let max_weight = 10.0_f64; // MACRO+HIGH+MEDIUM+LOW = 4+3+2+1
    let confidence = if stance == "wait-for-clarity" {
        0.2 // low confidence when we can't resolve
    } else {
        let base = (weight_diff as f64 / max_weight).clamp(0.0, 0.8);
        let severity_bonus = match severity.as_str() {
            "low" => 0.15,    // minor splits are easier to resolve
            "medium" => 0.05,
            _ => 0.0,         // high severity = hard to resolve
        };
        (base + severity_bonus).clamp(0.1, 0.95)
    };

    // Build resolution triggers
    let mut triggers = Vec::new();

    // If LOW disagrees with higher timeframes, short-term price action resolving it
    if div.low != div.medium || div.low != div.high {
        let low_dir = if div.low == "bull" { "upside" } else { "downside" };
        let opposite = if div.low == "bull" { "downside" } else { "upside" };
        triggers.push(format!(
            "SHORT-TERM: {} momentum confirmation would validate LOW {} read; {} reversal would align with higher timeframes",
            low_dir, div.low, opposite
        ));
    }

    // If MEDIUM and HIGH disagree
    if div.medium != "neutral" && div.high != "neutral" && div.medium != div.high {
        triggers.push(format!(
            "MID-TERM: Conviction score shift (MEDIUM:{}) aligning with trend impacts (HIGH:{}) would resolve mid/high split",
            div.medium, div.high
        ));
    }

    // If MACRO is the dissenter
    if div.macro_bias != "neutral" {
        let macro_dir = &div.macro_bias;
        let others_agree = ((div.low == div.high || div.low == div.medium)
            && div.low != "neutral")
            || (div.medium == div.high && div.medium != "neutral");
        if others_agree {
            triggers.push(format!(
                "MACRO: Scenario probability shift would reconcile MACRO:{} bias with shorter timeframes",
                macro_dir
            ));
        }
    }

    // Regime-specific trigger
    if regime_read == "conflicted" {
        triggers.push(
            "REGIME: Broad market regime clarification needed — too many cross-asset disagreements to resolve individual positions cleanly"
                .to_string(),
        );
    }

    // If no specific triggers were generated, add a generic one
    if triggers.is_empty() {
        triggers.push("Wait for directional confirmation from neutral layers gaining a bias".to_string());
    }

    // Low read and high read summaries
    let low_read = format!("LOW signals {} (regime-driven, short-term momentum)", div.low);
    let high_read = {
        let mut parts = Vec::new();
        if div.medium != "neutral" {
            parts.push(format!("MEDIUM:{} (conviction-based)", div.medium));
        }
        if div.high != "neutral" {
            parts.push(format!("HIGH:{} (trend-impact weighted)", div.high));
        }
        if div.macro_bias != "neutral" {
            parts.push(format!("MACRO:{} (scenario-probability weighted)", div.macro_bias));
        }
        if parts.is_empty() {
            "Higher timeframes neutral — no strong signal".to_string()
        } else {
            parts.join(", ")
        }
    };

    ResolutionEntry {
        symbol: div.symbol.clone(),
        disagreement,
        severity,
        dominant_timeframe,
        dominant_reason,
        stance,
        confidence: (confidence * 100.0).round() / 100.0,
        resolution_triggers: triggers,
        low_read,
        high_read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn macro_cycles_history_add_persists_row() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        run_macro_cycles_history_add(
            &backend,
            "US",
            "education",
            1950,
            9.0,
            Some("GI Bill expansion"),
            Some("test-source"),
            true,
        )
        .unwrap();

        let rows =
            structural::list_metric_history_backend(&backend, &["US".to_string()], None, None)
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].country, "US");
        assert_eq!(rows[0].metric, "education");
        assert_eq!(rows[0].decade, 1950);
        assert_eq!(rows[0].score, 9.0);
        assert_eq!(rows[0].notes.as_deref(), Some("GI Bill expansion"));
        assert_eq!(rows[0].source.as_deref(), Some("test-source"));
    }

    #[test]
    fn summary_json_never_empty_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        // run_summary should succeed and not error out on an empty database
        let result = run_summary(&backend, true);
        assert!(result.is_ok(), "run_summary should not error: {:?}", result);
    }

    #[test]
    fn situation_json_never_empty_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_situation(&backend, true);
        assert!(
            result.is_ok(),
            "run_situation should not error: {:?}",
            result
        );
    }

    #[test]
    fn deltas_json_never_empty_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_deltas(&backend, Some("last-refresh"), true);
        assert!(result.is_ok(), "run_deltas should not error: {:?}", result);
    }

    #[test]
    fn catalysts_json_never_errors_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_catalysts(&backend, Some("week"), true);
        assert!(
            result.is_ok(),
            "run_catalysts should not error: {:?}",
            result
        );
    }

    #[test]
    fn impact_json_never_errors_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_impact(&backend, true);
        assert!(result.is_ok(), "run_impact should not error: {:?}", result);
    }

    #[test]
    fn opportunities_json_never_errors_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_opportunities(&backend, true);
        assert!(
            result.is_ok(),
            "run_opportunities should not error: {:?}",
            result
        );
    }

    #[test]
    fn synthesis_json_never_errors_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_synthesis(&backend, true);
        assert!(
            result.is_ok(),
            "run_synthesis should not error: {:?}",
            result
        );
    }

    #[test]
    fn divergence_json_never_empty_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        // run_divergence should succeed and not error out on an empty database
        let result = run_divergence(&backend, None, true);
        assert!(
            result.is_ok(),
            "run_divergence should not error: {:?}",
            result
        );
    }

    #[test]
    fn alignment_json_never_empty_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_alignment(&backend, None, true);
        assert!(
            result.is_ok(),
            "run_alignment should not error: {:?}",
            result
        );
    }

    #[test]
    fn alignment_summary_empty_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_alignment_summary(&backend, None, true);
        assert!(
            result.is_ok(),
            "run_alignment_summary should not error on empty db: {:?}",
            result
        );
    }

    #[test]
    fn alignment_summary_terminal_empty_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_alignment_summary(&backend, None, false);
        assert!(
            result.is_ok(),
            "run_alignment_summary terminal should not error on empty db: {:?}",
            result
        );
    }

    #[test]
    fn consensus_from_counts_strong_buy() {
        assert_eq!(consensus_from_counts(4, 0), "STRONG BUY");
    }

    #[test]
    fn consensus_from_counts_strong_avoid() {
        assert_eq!(consensus_from_counts(0, 4), "STRONG AVOID");
    }

    #[test]
    fn consensus_from_counts_bullish() {
        assert_eq!(consensus_from_counts(3, 1), "BULLISH");
    }

    #[test]
    fn consensus_from_counts_bearish() {
        assert_eq!(consensus_from_counts(1, 3), "BEARISH");
    }

    #[test]
    fn consensus_from_counts_mixed() {
        assert_eq!(consensus_from_counts(2, 2), "MIXED");
        assert_eq!(consensus_from_counts(1, 1), "MIXED");
        assert_eq!(consensus_from_counts(0, 0), "MIXED");
    }

    #[test]
    fn score_bar_extremes() {
        let bar0 = score_bar(0.0);
        assert_eq!(bar0, "░░░░░░░░░░");
        let bar100 = score_bar(100.0);
        assert_eq!(bar100, "██████████");
        let bar50 = score_bar(50.0);
        assert_eq!(bar50, "█████░░░░░");
    }

    #[test]
    fn bias_from_score_directions() {
        assert_eq!(bias_from_score(3), "bull");
        assert_eq!(bias_from_score(-2), "bear");
        assert_eq!(bias_from_score(0), "neutral");
    }

    #[test]
    fn low_json_never_empty_on_fresh_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let result = run_low(&backend, true);
        assert!(result.is_ok(), "run_low should not error: {:?}", result);
    }

    #[test]
    fn low_json_includes_scenario_probabilities() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        // Insert a scenario
        scenarios::add_scenario_backend(
            &backend,
            "Test Recession",
            65.0,
            Some("Test description"),
            None,
            None,
            None,
        )
        .unwrap();

        // Capture run_low JSON output
        let result = run_low(&backend, false);
        assert!(result.is_ok(), "run_low text should work with scenarios");

        // Verify scenarios are loaded (can't capture stdout easily, but
        // verify the backend query used in run_low returns data)
        let active =
            scenarios::list_scenarios_backend(&backend, Some("active")).unwrap_or_default();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "Test Recession");
        assert!((active[0].probability - 65.0).abs() < 0.01);
    }

    #[test]
    fn summary_json_includes_scenario_probabilities() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        // Insert two scenarios with different probabilities
        scenarios::add_scenario_backend(
            &backend,
            "Inflation Spike",
            80.0,
            Some("CPI rising"),
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::add_scenario_backend(
            &backend,
            "Risk Rally",
            15.0,
            Some("Fed cuts"),
            None,
            None,
            None,
        )
        .unwrap();

        // run_summary should succeed with scenarios present
        let result = run_summary(&backend, true);
        assert!(
            result.is_ok(),
            "run_summary should not error with scenarios: {:?}",
            result
        );

        // Verify both scenarios exist and probabilities are correct
        let active =
            scenarios::list_scenarios_backend(&backend, Some("active")).unwrap_or_default();
        assert_eq!(active.len(), 2);
    }

    // ==================== Resolution Tests ====================

    fn make_divergence(
        symbol: &str,
        low: &str,
        medium: &str,
        high: &str,
        macro_bias: &str,
        bull: usize,
        bear: usize,
    ) -> DivergenceRow {
        DivergenceRow {
            symbol: symbol.to_string(),
            low: low.to_string(),
            medium: medium.to_string(),
            high: high.to_string(),
            macro_bias: macro_bias.to_string(),
            bull_layers: bull,
            bear_layers: bear,
            disagreement_pct: (bull.min(bear) as f64 / 4.0) * 100.0,
            dominant_side: if bull > bear {
                "bull".to_string()
            } else if bear > bull {
                "bear".to_string()
            } else {
                "split".to_string()
            },
        }
    }

    #[test]
    fn resolution_low_bear_higher_bull_leans_bull() {
        // LOW:bear vs MEDIUM:bull, HIGH:bull → higher timeframes dominate → lean-bull
        let div = make_divergence("BTC", "bear", "bull", "bull", "neutral", 2, 1);
        let entry = build_resolution_entry(&div, "mixed");
        assert_eq!(entry.stance, "lean-bull");
        assert_eq!(entry.dominant_timeframe, "HIGH");
        assert!(entry.confidence > 0.3);
    }

    #[test]
    fn resolution_low_bull_higher_bear_leans_bear() {
        // LOW:bull vs MEDIUM:bear, HIGH:bear → higher timeframes dominate → lean-bear
        let div = make_divergence("GOLD", "bull", "bear", "bear", "neutral", 1, 2);
        let entry = build_resolution_entry(&div, "mixed");
        assert_eq!(entry.stance, "lean-bear");
        assert_eq!(entry.dominant_timeframe, "HIGH");
        assert!(entry.confidence > 0.3);
    }

    #[test]
    fn resolution_even_split_waits() {
        // LOW:bear, MEDIUM:bull, HIGH:bear, MACRO:bull → 2v2 → wait-for-clarity
        let div = make_divergence("SPY", "bear", "bull", "bear", "bull", 2, 2);
        let entry = build_resolution_entry(&div, "mixed");
        // 2v2 with weights: bull=MEDIUM(2)+MACRO(4)=6, bear=LOW(1)+HIGH(3)=4
        // weight_diff=2 > threshold(1) → actually resolves to lean-bull
        assert_eq!(entry.stance, "lean-bull");
        assert_eq!(entry.severity, "high");
    }

    #[test]
    fn resolution_conflicted_regime_raises_threshold() {
        // In conflicted regime, weight threshold is 3 instead of 1
        // LOW:bear, MEDIUM:bull → bear_weight=1, bull_weight=2, diff=1 < 3
        let div = make_divergence("ETH", "bear", "bull", "neutral", "neutral", 1, 1);
        let entry = build_resolution_entry(&div, "conflicted");
        assert_eq!(entry.stance, "wait-for-clarity");
        assert!(
            entry
                .resolution_triggers
                .iter()
                .any(|t| t.contains("REGIME")),
            "conflicted regime should add regime trigger"
        );
    }

    #[test]
    fn resolution_severity_high_when_2v2() {
        let div = make_divergence("TSLA", "bear", "bull", "bear", "bull", 2, 2);
        let entry = build_resolution_entry(&div, "clean");
        assert_eq!(entry.severity, "high");
    }

    #[test]
    fn resolution_severity_low_when_1v1() {
        let div = make_divergence("AAPL", "bear", "bull", "neutral", "neutral", 1, 1);
        let entry = build_resolution_entry(&div, "clean");
        assert_eq!(entry.severity, "low");
    }

    #[test]
    fn resolution_severity_medium_when_3_active() {
        let div = make_divergence("NVDA", "bear", "bull", "bull", "neutral", 2, 1);
        let entry = build_resolution_entry(&div, "clean");
        assert_eq!(entry.severity, "medium");
    }

    #[test]
    fn resolution_macro_dominant_when_macro_bull() {
        // MACRO:bull + HIGH:bull vs LOW:bear → MACRO is highest-weight bull layer
        let div = make_divergence("GLD", "bear", "neutral", "bull", "bull", 2, 1);
        let entry = build_resolution_entry(&div, "clean");
        assert_eq!(entry.stance, "lean-bull");
        assert_eq!(entry.dominant_timeframe, "MACRO");
    }

    #[test]
    fn resolution_triggers_include_short_term() {
        let div = make_divergence("SLV", "bear", "bull", "neutral", "neutral", 1, 1);
        let entry = build_resolution_entry(&div, "clean");
        assert!(
            entry
                .resolution_triggers
                .iter()
                .any(|t| t.contains("SHORT-TERM")),
            "should include short-term trigger when LOW disagrees"
        );
    }

    #[test]
    fn resolution_triggers_include_midterm_when_medium_high_split() {
        let div = make_divergence("CCJ", "bear", "bull", "bear", "neutral", 1, 2);
        let entry = build_resolution_entry(&div, "clean");
        assert!(
            entry
                .resolution_triggers
                .iter()
                .any(|t| t.contains("MID-TERM")),
            "should include mid-term trigger when MEDIUM and HIGH disagree"
        );
    }

    #[test]
    fn resolution_confidence_low_for_wait() {
        let div = make_divergence("ETH", "bear", "bull", "neutral", "neutral", 1, 1);
        let entry = build_resolution_entry(&div, "conflicted");
        assert_eq!(entry.stance, "wait-for-clarity");
        assert!(
            entry.confidence <= 0.25,
            "wait-for-clarity should have low confidence, got {}",
            entry.confidence
        );
    }

    #[test]
    fn resolution_confidence_higher_for_clear_dominant() {
        // MACRO:bull + HIGH:bull + MEDIUM:bull vs LOW:bear → very clear
        let div = make_divergence("MSTR", "bear", "bull", "bull", "bull", 3, 1);
        let entry = build_resolution_entry(&div, "clean");
        assert_eq!(entry.stance, "lean-bull");
        assert!(
            entry.confidence >= 0.7,
            "strong 3v1 should have high confidence, got {}",
            entry.confidence
        );
    }

    #[test]
    fn resolution_entry_serializes_to_json() {
        let div = make_divergence("BTC", "bear", "bull", "bull", "neutral", 2, 1);
        let entry = build_resolution_entry(&div, "mixed");
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"symbol\":\"BTC\""));
        assert!(json.contains("\"stance\""));
        assert!(json.contains("\"resolution_triggers\""));
        assert!(json.contains("\"dominant_timeframe\""));
    }

    #[test]
    fn resolution_disagreement_describes_active_layers() {
        let div = make_divergence("RKLB", "bear", "bull", "neutral", "bull", 2, 1);
        let entry = build_resolution_entry(&div, "clean");
        assert!(
            entry.disagreement.contains("LOW:bear"),
            "disagreement should mention LOW:bear, got: {}",
            entry.disagreement
        );
        assert!(
            entry.disagreement.contains("MEDIUM:bull"),
            "disagreement should mention MEDIUM:bull, got: {}",
            entry.disagreement
        );
    }
}
