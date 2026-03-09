use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::Connection;
use serde_json::json;

use crate::db::structural;

#[allow(clippy::too_many_arguments)]
pub fn run(
    conn: &Connection,
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
        "metric-set" => {
            let c = country.or(value).ok_or_else(|| anyhow::anyhow!("--country required"))?;
            let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
            let id = structural::set_metric(conn, c, m, score, rank, trend, notes, source)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Set power metric #{} {}:{}", id, c, m);
            }
        }
        "metric-list" => {
            let rows = structural::list_metrics(conn, country, metric)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "metrics": rows }))?);
            } else {
                println!("Power metrics ({}):", rows.len());
                for r in rows {
                    println!(
                        "  {} {} score={:?} rank={:?} trend={} ({})",
                        r.country, r.metric, r.score, r.rank, r.trend, r.recorded_at
                    );
                }
            }
        }
        "metric-history" => {
            let c = country.or(value).ok_or_else(|| anyhow::anyhow!("country required"))?;
            let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
            let rows = structural::get_metric_history(conn, c, m, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "history": rows }))?);
            } else {
                println!("Metric history {}:{} ({})", c, m, rows.len());
                for r in rows {
                    println!("  {} score={:?} rank={:?}", r.recorded_at, r.score, r.rank);
                }
            }
        }

        "cycle-set" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("cycle name required"))?;
            let stg = stage.ok_or_else(|| anyhow::anyhow!("--stage required"))?;
            structural::set_cycle(conn, name, stg, entered, description, evidence)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "updated": name }))?);
            } else {
                println!("Set structural cycle {}", name);
            }
        }
        "cycle-list" => {
            let rows = structural::list_cycles(conn)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "cycles": rows }))?);
            } else {
                println!("Structural cycles ({}):", rows.len());
                for r in rows {
                    println!("  {}: {} (since {:?})", r.cycle_name, r.current_stage, r.stage_entered);
                }
            }
        }

        "outcome-add" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("outcome name required"))?;
            let p = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
            let id = structural::add_outcome(conn, name, p, horizon, description, parallel, impact, signals)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Added structural outcome #{} {}", id, name);
            }
        }
        "outcome-list" => {
            let rows = structural::list_outcomes(conn)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "outcomes": rows }))?);
            } else {
                println!("Structural outcomes ({}):", rows.len());
                for r in rows {
                    println!("  {}: {:.1}% ({})", r.name, r.probability, r.status);
                }
            }
        }
        "outcome-update" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("outcome name required"))?;
            let p = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
            structural::update_outcome_probability(conn, name, p, driver)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "updated": name, "probability": p }))?);
            } else {
                println!("Updated structural outcome {} to {:.1}%", name, p);
            }
        }
        "outcome-history" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("outcome name required"))?;
            let rows = structural::get_outcome_history(conn, name, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "history": rows }))?);
            } else {
                println!("Outcome history {}:", name);
                for (prob, drv, ts) in rows {
                    println!("  {}  {:.1}%  {:?}", ts, prob, drv);
                }
            }
        }

        "parallel-add" => {
            let p = period.ok_or_else(|| anyhow::anyhow!("--period required"))?;
            let ev = event.ok_or_else(|| anyhow::anyhow!("--event required"))?;
            let pt = parallel_to.ok_or_else(|| anyhow::anyhow!("--parallel-to required"))?;
            let id = structural::add_parallel(conn, p, ev, pt, similarity, outcome, notes, source)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Added historical parallel #{}", id);
            }
        }
        "parallel-list" => {
            let rows = structural::list_parallels(conn, period)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "parallels": rows }))?);
            } else {
                println!("Historical parallels ({}):", rows.len());
                for r in rows {
                    println!("  {} | {} -> {}", r.period, r.event, r.parallel_to);
                }
            }
        }
        "parallel-search" => {
            let q = value.ok_or_else(|| anyhow::anyhow!("search query required"))?;
            let rows = structural::search_parallels(conn, q)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "parallels": rows }))?);
            } else {
                println!("Parallel search '{}' ({}):", q, rows.len());
                for r in rows {
                    println!("  {} | {}", r.period, r.event);
                }
            }
        }

        "log-add" => {
            let d = date
                .map(|x| x.to_string())
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
            let dev = value.ok_or_else(|| anyhow::anyhow!("development text required"))?;
            let id = structural::add_log(conn, &d, dev, impact, outcome)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
            } else {
                println!("Added structural log #{}", id);
            }
        }
        "log-list" => {
            let rows = structural::list_log(conn, since, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "log": rows }))?);
            } else {
                println!("Structural log ({}):", rows.len());
                for r in rows {
                    println!("  {} | {}", r.date, r.development);
                }
            }
        }

        "dashboard" => {
            let cycles = structural::list_cycles(conn)?;
            let outcomes = structural::list_outcomes(conn)?;
            let metrics = structural::list_metrics(conn, None, None)?;
            let log_rows = structural::list_log(conn, None, Some(5))?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "cycles": cycles,
                        "outcomes": outcomes,
                        "metrics": metrics,
                        "recent_log": log_rows,
                    }))?
                );
            } else {
                println!("Structural Dashboard");
                println!("════════════════════════════════════════════════════");
                println!("\nCycles:");
                for c in cycles {
                    println!("  {:<24} {:<28} since {:?}", c.cycle_name, c.current_stage, c.stage_entered);
                }
                println!("\nStructural Outcomes:");
                for o in outcomes.into_iter().take(8) {
                    println!("  {:<28} {:>5.1}%", o.name, o.probability);
                }
                println!("\nRecent Log:");
                for r in log_rows {
                    println!("  {}  {}", r.date, r.development);
                }
            }
        }

        _ => bail!("unknown structural action '{}'. See --help", action),
    }

    Ok(())
}
