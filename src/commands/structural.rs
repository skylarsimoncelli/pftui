#![allow(dead_code)]

use crate::db::backend::BackendConnection;
use crate::db::structural;
use anyhow::Result;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    country: Option<&str>,
    metric: Option<&str>,
    score: Option<f64>,
    rank: Option<i32>,
    trend: Option<&str>,
    stage: Option<&str>,
    entered: Option<&str>,
    probability: Option<f64>,
    horizon: Option<&str>,
    description: Option<&str>,
    parallel: Option<&str>,
    impact: Option<&str>,
    driver: Option<&str>,
    period: Option<&str>,
    event: Option<&str>,
    parallel_to: Option<&str>,
    similarity: Option<i32>,
    outcome: Option<&str>,
    evidence: Option<&str>,
    signals: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
    date: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        // Power metrics
        "metric-set" => {
            let c = country.ok_or_else(|| anyhow::anyhow!("--country required"))?;
            let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
            let t = trend.unwrap_or("stable");
            let id = structural::set_metric_backend(backend, c, m, score, rank, t, notes, source)?;
            if json_output {
                println!("{}", json!({"id": id, "country": c, "metric": m}));
            } else {
                println!("Set metric: {} {} = {:?}", c, m, score);
            }
        }
        "metric-list" => {
            let metrics = structural::list_metrics_backend(backend, country, metric)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&metrics)?);
            } else {
                for m in &metrics {
                    println!(
                        "{} {} = {:?} (rank {}) {} — {}",
                        m.country,
                        m.metric,
                        m.score,
                        m.rank.map_or("?".to_string(), |r| r.to_string()),
                        m.trend,
                        m.source.as_ref().unwrap_or(&"—".to_string())
                    );
                }
                println!("\nTotal: {} metrics", metrics.len());
            }
        }
        "metric-history" => {
            let c = country.ok_or_else(|| anyhow::anyhow!("--country required"))?;
            let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
            let history = structural::get_metric_history_backend(backend, c, m, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&history)?);
            } else {
                println!("History: {} {}", c, m);
                for h in &history {
                    println!(
                        "  {} | {:?} (rank {}) {} — {}",
                        h.recorded_at,
                        h.score,
                        h.rank.map_or("?".to_string(), |r| r.to_string()),
                        h.trend,
                        h.source.as_ref().unwrap_or(&"—".to_string())
                    );
                }
            }
        }

        // Cycles
        "cycle-set" => {
            let name =
                value.ok_or_else(|| anyhow::anyhow!("cycle name required as first argument"))?;
            let s = stage.ok_or_else(|| anyhow::anyhow!("--stage required"))?;
            structural::set_cycle_backend(backend, name, s, entered, description, evidence)?;
            if json_output {
                println!("{}", json!({"name": name, "stage": s}));
            } else {
                println!("Set cycle: {} → {}", name, s);
            }
        }
        "cycle-list" => {
            let cycles = structural::list_cycles_backend(backend)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&cycles)?);
            } else {
                for c in &cycles {
                    println!(
                        "{}: {} (since {})",
                        c.cycle_name,
                        c.current_stage,
                        c.stage_entered.as_ref().unwrap_or(&"?".to_string())
                    );
                    if let Some(desc) = &c.description {
                        println!("  {}", desc);
                    }
                }
            }
        }

        // Outcomes
        "outcome-add" => {
            let name =
                value.ok_or_else(|| anyhow::anyhow!("outcome name required as first argument"))?;
            let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
            let id = structural::add_outcome_backend(
                backend,
                name,
                prob,
                horizon,
                description,
                parallel,
                outcome,
                signals,
            )?;
            if json_output {
                println!("{}", json!({"id": id, "name": name, "probability": prob}));
            } else {
                println!("Added outcome: {} ({}%)", name, prob);
            }
        }
        "outcome-list" => {
            let outcomes = structural::list_outcomes_backend(backend)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&outcomes)?);
            } else {
                for o in &outcomes {
                    println!("{}: {:.0}%", o.name, o.probability);
                    if let Some(h) = &o.time_horizon {
                        println!("  Horizon: {}", h);
                    }
                    if let Some(p) = &o.historical_parallel {
                        println!("  Parallel: {}", p);
                    }
                }
            }
        }
        "outcome-update" => {
            let name =
                value.ok_or_else(|| anyhow::anyhow!("outcome name required as first argument"))?;
            let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
            structural::update_outcome_probability_backend(backend, name, prob, driver)?;
            if json_output {
                println!(
                    "{}",
                    json!({"name": name, "probability": prob, "driver": driver})
                );
            } else {
                println!("Updated {} to {:.0}%", name, prob);
            }
        }
        "outcome-history" => {
            let name =
                value.ok_or_else(|| anyhow::anyhow!("outcome name required as first argument"))?;
            let history = structural::get_outcome_history_backend(backend, name, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&history)?);
            } else {
                println!("History: {}", name);
                for (prob, drv, recorded) in &history {
                    println!(
                        "  {} | {:.0}% — {}",
                        recorded,
                        prob,
                        drv.as_ref().unwrap_or(&"—".to_string())
                    );
                }
            }
        }

        // Parallels
        "parallel-add" => {
            let p = period.ok_or_else(|| anyhow::anyhow!("--period required"))?;
            let e = event.ok_or_else(|| anyhow::anyhow!("--event required"))?;
            let pt = parallel_to.ok_or_else(|| anyhow::anyhow!("--parallel-to required"))?;
            let id = structural::add_parallel_backend(
                backend, p, e, pt, similarity, outcome, notes, source,
            )?;
            if json_output {
                println!("{}", json!({"id": id, "period": p, "event": e}));
            } else {
                println!("Added parallel: {} → {}", e, pt);
            }
        }
        "parallel-list" => {
            let parallels = structural::list_parallels_backend(backend, period)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&parallels)?);
            } else {
                for p in &parallels {
                    println!("{} | {} → {}", p.period, p.event, p.parallel_to);
                    if let Some(score) = p.similarity_score {
                        println!("  Similarity: {}/10", score);
                    }
                    if let Some(out) = &p.asset_outcome {
                        println!("  Outcome: {}", out);
                    }
                }
            }
        }
        "parallel-search" => {
            let query =
                value.ok_or_else(|| anyhow::anyhow!("search query required as first argument"))?;
            let results = structural::search_parallels_backend(backend, query)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for r in &results {
                    println!("{} | {} → {}", r.period, r.event, r.parallel_to);
                }
                println!("\nFound {} parallels", results.len());
            }
        }

        // Log
        "log-add" => {
            let dev = value
                .ok_or_else(|| anyhow::anyhow!("development text required as first argument"))?;
            let d = date.ok_or_else(|| anyhow::anyhow!("--date required"))?;
            // CLI: --impact → cycle_impact, --outcome → outcome_shift
            let id = structural::add_log_backend(backend, d, dev, impact, outcome)?;
            if json_output {
                println!("{}", json!({"id": id, "date": d}));
            } else {
                println!("Added log entry for {}", d);
            }
        }
        "log-list" => {
            let logs = structural::list_log_backend(backend, since, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&logs)?);
            } else {
                for l in &logs {
                    println!("\n{} | {}", l.date, l.development);
                    if let Some(impact) = &l.cycle_impact {
                        println!("  Cycle: {}", impact);
                    }
                    if let Some(shift) = &l.outcome_shift {
                        println!("  Shift: {}", shift);
                    }
                }
            }
        }

        // Dashboard
        "dashboard" => {
            run_dashboard(backend, json_output)?;
        }

        _ => anyhow::bail!("Unknown action: {}", action),
    }

    Ok(())
}

fn run_dashboard(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let cycles = structural::list_cycles_backend(backend)?;
    let metrics = structural::list_metrics_backend(backend, None, None)?;
    let outcomes = structural::list_outcomes_backend(backend)?;
    let logs = structural::list_log_backend(backend, None, Some(4))?;

    if json_output {
        let dashboard = json!({
            "cycles": cycles,
            "power_metrics": metrics,
            "structural_outcomes": outcomes,
            "recent_log": logs,
        });
        println!("{}", serde_json::to_string_pretty(&dashboard)?);
    } else {
        println!("Structural Dashboard");
        println!("════════════════════════════════════════════════════════════════\n");

        // Cycles
        if !cycles.is_empty() {
            println!("Cycles:");
            for c in &cycles {
                println!(
                    "  {:28} {:30} since {}",
                    c.cycle_name,
                    c.current_stage,
                    c.stage_entered.as_ref().unwrap_or(&"?".to_string())
                );
            }
            println!();
        }

        // Power Metrics (group by country)
        if !metrics.is_empty() {
            println!("Power Metrics:");
            let mut by_country: std::collections::HashMap<String, Vec<_>> =
                std::collections::HashMap::new();
            for m in &metrics {
                by_country.entry(m.country.clone()).or_default().push(m);
            }

            let mut countries: Vec<_> = by_country.keys().collect();
            countries.sort();

            for country in countries {
                println!("  {}:", country);
                let mut country_metrics = by_country[country].clone();
                country_metrics.sort_by(|a, b| a.metric.cmp(&b.metric));

                for m in country_metrics {
                    let trend_arrow = match m.trend.as_str() {
                        "rising" => "↗",
                        "declining" => "↘",
                        _ => "→",
                    };
                    println!(
                        "    {:20} {:?} (rank {}) {}",
                        m.metric,
                        m.score,
                        m.rank.map_or("?".to_string(), |r| r.to_string()),
                        trend_arrow
                    );
                }
            }
            println!();
        }

        // Structural Outcomes
        if !outcomes.is_empty() {
            println!("Structural Outcomes (10-30yr):");
            for o in &outcomes {
                let parallel_text = o
                    .historical_parallel
                    .as_ref()
                    .map(|p| format!("  parallel: {}", p))
                    .unwrap_or_default();
                println!("  {:35} {:4.0}%{}", o.name, o.probability, parallel_text);
            }
            println!();
        }

        // Recent log
        if !logs.is_empty() {
            println!("Recent (last {} weeks):", logs.len());
            for l in &logs {
                println!("  {}  {}", l.date, l.development);
            }
        }
    }

    Ok(())
}
