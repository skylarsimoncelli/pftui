use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::{
    convictions, correlation_snapshots, regime_snapshots, research_questions, scenarios, structural,
    thesis, timeframe_signals, trends, user_predictions,
};

pub fn run(
    backend: &BackendConnection,
    action: &str,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    if action == "signals" {
        return run_signals(backend, symbol, signal_type, severity, limit, json_output);
    }
    let Some(conn) = backend.sqlite_native() else {
        bail!(
            "analytics '{}' currently requires database_backend=sqlite",
            action
        );
    };
    match action {
        "summary" => run_summary(conn, json_output),
        "low" => run_low(conn, json_output),
        "medium" => run_medium(conn, json_output),
        "high" => run_high(conn, json_output),
        "macro" => run_macro(conn, json_output),
        "alignment" => run_alignment(conn, symbol, json_output),
        _ => bail!(
            "unknown analytics action '{}'. Valid: signals, summary, low, medium, high, macro, alignment",
            action
        ),
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
    let mut rows =
        timeframe_signals::list_signals_backend(backend, signal_type, severity, limit.or(Some(25)))?;
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
                sig.severity, sig.signal_type, sig.description, sig.assets, sig.layers, sig.detected_at
            );
        }
    }

    Ok(())
}

fn run_summary(conn: &Connection, json_output: bool) -> Result<()> {
    let regime = regime_snapshots::get_current(conn)?;
    let scenarios_list = scenarios::list_scenarios(conn, Some("active")).unwrap_or_default();
    let top_scenario = scenarios_list.first().cloned();
    let trends_list = trends::list_trends(conn, Some("active"), None).unwrap_or_default();
    let top_trend = trends_list.first().cloned();
    let cycles = structural::list_cycles(conn).unwrap_or_default();
    let top_cycle = cycles.first().cloned();
    let signal = timeframe_signals::latest_signal(conn)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "regime": regime,
                "top_scenario": top_scenario,
                "top_trend": top_trend,
                "top_cycle": top_cycle,
                "top_signal": signal,
            }))?
        );
    } else {
        println!("Analytics Engine — Multi-Timeframe Intelligence");
        println!("════════════════════════════════════════════════════════════════");
        if let Some(r) = regime {
            println!("LOW: {} ({:.2})", r.regime.to_uppercase(), r.confidence.unwrap_or(0.0));
        } else {
            println!("LOW: no regime snapshot");
        }
        if let Some(s) = top_scenario {
            println!("MEDIUM: {} ({:.1}%)", s.name, s.probability);
        } else {
            println!("MEDIUM: no active scenario");
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
    }
    Ok(())
}

fn run_low(conn: &Connection, json_output: bool) -> Result<()> {
    let regime = regime_snapshots::get_current(conn)?;
    let corr = correlation_snapshots::list_current(conn, Some("30d")).unwrap_or_default();
    let signals = timeframe_signals::list_signals(conn, None, None, Some(10)).unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "regime": regime,
                "correlations": corr,
                "signals": signals,
            }))?
        );
    } else {
        println!("LOW Layer");
        if let Some(r) = regime {
            println!("  Regime: {} ({:.2})", r.regime, r.confidence.unwrap_or(0.0));
        }
        println!("  Correlations tracked: {}", corr.len());
        println!("  Signals: {}", signals.len());
    }

    Ok(())
}

fn run_medium(conn: &Connection, json_output: bool) -> Result<()> {
    let scenarios_list = scenarios::list_scenarios(conn, Some("active")).unwrap_or_default();
    let thesis_sections = thesis::list_thesis(conn).unwrap_or_default();
    let conviction_rows = convictions::list_current(conn).unwrap_or_default();
    let questions = research_questions::list_questions(conn, Some("open")).unwrap_or_default();
    let predictions = user_predictions::list_predictions(conn, Some("pending"), None, Some(20)).unwrap_or_default();

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

fn run_high(conn: &Connection, json_output: bool) -> Result<()> {
    let trends_list = trends::list_trends(conn, Some("active"), None).unwrap_or_default();
    let mut evidence = Vec::new();
    let mut impacts = Vec::new();
    for t in &trends_list {
        let ev = trends::list_evidence(conn, t.id, Some(3)).unwrap_or_default();
        evidence.push(json!({ "trend": t.name, "items": ev }));
        let imp = trends::list_asset_impacts(conn, t.id).unwrap_or_default();
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

fn run_macro(conn: &Connection, json_output: bool) -> Result<()> {
    let metrics = structural::list_metrics(conn, None, None).unwrap_or_default();
    let cycles = structural::list_cycles(conn).unwrap_or_default();
    let outcomes = structural::list_outcomes(conn).unwrap_or_default();
    let parallels = structural::list_parallels(conn, None).unwrap_or_default();
    let log_rows = structural::list_log(conn, None, Some(10)).unwrap_or_default();

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

fn regime_to_bias(regime: &str) -> &'static str {
    match regime {
        "risk-on" => "bull",
        "risk-off" | "crisis" | "stagflation" => "bear",
        _ => "neutral",
    }
}

fn run_alignment(conn: &Connection, symbol: Option<&str>, json_output: bool) -> Result<()> {
    let sym = symbol.unwrap_or("GC=F").to_uppercase();

    let low = regime_snapshots::get_current(conn)?
        .map(|r| regime_to_bias(&r.regime).to_string())
        .unwrap_or_else(|| "neutral".to_string());

    let medium = convictions::list_current(conn)
        .unwrap_or_default()
        .into_iter()
        .find(|c| c.symbol.to_uppercase() == sym)
        .map(|c| {
            if c.score > 0 {
                "bull".to_string()
            } else if c.score < 0 {
                "bear".to_string()
            } else {
                "neutral".to_string()
            }
        })
        .unwrap_or_else(|| "neutral".to_string());

    let high_impacts = trends::get_impacts_for_symbol(conn, &sym).unwrap_or_default();
    let bull_high = high_impacts.iter().filter(|(_, i)| i.impact == "bullish").count();
    let bear_high = high_impacts.iter().filter(|(_, i)| i.impact == "bearish").count();
    let high = if bull_high > bear_high {
        "bull"
    } else if bear_high > bull_high {
        "bear"
    } else {
        "neutral"
    }
    .to_string();

    let macro_outcomes = structural::list_outcomes(conn).unwrap_or_default();
    let mut macro_bias = "neutral".to_string();
    for o in macro_outcomes {
        if let Some(ai) = o.asset_implications.as_ref() {
            let lower = ai.to_lowercase();
            if lower.contains(&sym.to_lowercase()) {
                if lower.contains("bull") {
                    macro_bias = "bull".to_string();
                }
                if lower.contains("bear") {
                    macro_bias = "bear".to_string();
                }
                break;
            }
        }
    }

    let layers = vec![
        ("low", low.clone()),
        ("medium", medium.clone()),
        ("high", high.clone()),
        ("macro", macro_bias.clone()),
    ];
    let bull = layers.iter().filter(|(_, v)| v == "bull").count();
    let bear = layers.iter().filter(|(_, v)| v == "bear").count();

    let consensus = if bull == 4 {
        "STRONG BUY"
    } else if bear == 4 {
        "STRONG AVOID"
    } else if bull >= 3 {
        "BULLISH"
    } else if bear >= 3 {
        "BEARISH"
    } else {
        "MIXED"
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "symbol": sym,
                "layers": {
                    "low": low,
                    "medium": medium,
                    "high": high,
                    "macro": macro_bias,
                },
                "consensus": consensus,
            }))?
        );
    } else {
        println!("Alignment for {}", sym);
        for (layer, bias) in layers {
            println!("  {:<6} {}", layer, bias);
        }
        println!("  consensus: {}", consensus);
    }

    Ok(())
}
