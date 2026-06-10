//! `pftui analytics epistemics` — run-health instrumentation (epistemics R4).
//!
//! Subcommands:
//!   record  — upsert one run's epistemic-health metrics by date
//!   show    — one run's row with threshold flags
//!   history — newest-first trend table
//!   rivalry — house-vs-antithesis scored-prediction scoreboard

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::run_health::{self, RunHealth, RunHealthInput};

fn sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("analytics epistemics requires the SQLite backend"))
}

fn validate_date(date: &str) -> Result<()> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("invalid --date '{}': expected YYYY-MM-DD", date))
}

fn validate_unit_interval(name: &str, value: Option<f64>) -> Result<()> {
    if let Some(v) = value {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            anyhow::bail!("{} must be in 0..=1, got {}", name, v);
        }
    }
    Ok(())
}

/// Record (upsert) a run's health metrics. Derives what it can when flags
/// are omitted: blind_divergence from same-day analyst views,
/// scenario_delta_total from today's scenario probability ledger.
#[allow(clippy::too_many_arguments)]
pub fn record(
    backend: &BackendConnection,
    date: &str,
    agreement: Option<f64>,
    blind_divergence: Option<f64>,
    panel_dispersion: Option<f64>,
    novelty: Option<f64>,
    fallback_warnings: Option<i64>,
    scenario_delta_total: Option<f64>,
    audit_pass_rate: Option<f64>,
    agents: Option<i64>,
    notes: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = sqlite(backend)?;
    validate_date(date)?;
    validate_unit_interval("--agreement", agreement)?;
    validate_unit_interval("--novelty", novelty)?;
    validate_unit_interval("--audit-pass-rate", audit_pass_rate)?;

    let mut derived: Vec<&str> = Vec::new();
    let blind_divergence = match blind_divergence {
        Some(v) => Some(v),
        None => {
            let computed = run_health::compute_blind_divergence(conn, date)?;
            if computed.is_some() {
                derived.push("blind_divergence");
            }
            computed
        }
    };
    let scenario_delta_total = match scenario_delta_total {
        Some(v) => Some(v),
        None => {
            derived.push("scenario_delta_total");
            Some(crate::db::scenarios::scenario_delta_total_for_day(
                conn, date,
            )?)
        }
    };

    let input = RunHealthInput {
        agreement_rate: agreement,
        blind_divergence,
        panel_dispersion,
        novelty_rate: novelty,
        fallback_warnings,
        scenario_delta_total,
        audit_pass_rate,
        agents_spawned: agents,
        notes: notes.map(str::to_string),
    };
    let id = run_health::upsert_run_health(conn, date, &input)?;
    let row = run_health::get_run_health(conn, date)?
        .ok_or_else(|| anyhow::anyhow!("run_health row vanished after upsert"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "run_health_recorded",
                "id": id,
                "derived": derived,
                "row": row,
                "flags": flags_json(&row),
            }))?
        );
    } else {
        println!("Recorded run health for {} (id {}).", date, id);
        if !derived.is_empty() {
            println!("  derived by pftui: {}", derived.join(", "));
        }
        print_row_text(&row);
    }
    Ok(())
}

fn flags_json(row: &RunHealth) -> Vec<serde_json::Value> {
    run_health::threshold_flags(row)
        .into_iter()
        .map(|(metric, warning)| json!({ "metric": metric, "warning": warning }))
        .collect()
}

fn fmt_opt_f64(v: Option<f64>, precision: usize) -> String {
    v.map(|x| format!("{:.*}", precision, x))
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_opt_i64(v: Option<i64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "—".to_string())
}

fn print_row_text(row: &RunHealth) {
    let flags: std::collections::BTreeMap<&str, String> =
        run_health::threshold_flags(row).into_iter().collect();
    let flag_for = |metric: &str| flags.get(metric).cloned().unwrap_or_default();

    println!("Run health — {}", row.run_date);
    println!("  {:<22} {:>8}  flag", "metric", "value");
    println!("  {}", "-".repeat(56));
    println!(
        "  {:<22} {:>8}  {}",
        "agreement_rate",
        fmt_opt_f64(row.agreement_rate, 2),
        flag_for("agreement_rate"),
    );
    println!(
        "  {:<22} {:>8}  {}",
        "blind_divergence",
        fmt_opt_f64(row.blind_divergence, 2),
        flag_for("blind_divergence"),
    );
    println!(
        "  {:<22} {:>8}  {}",
        "panel_dispersion",
        fmt_opt_f64(row.panel_dispersion, 1),
        flag_for("panel_dispersion"),
    );
    println!(
        "  {:<22} {:>8}",
        "novelty_rate",
        fmt_opt_f64(row.novelty_rate, 2),
    );
    println!(
        "  {:<22} {:>8}",
        "fallback_warnings",
        fmt_opt_i64(row.fallback_warnings),
    );
    println!(
        "  {:<22} {:>8}",
        "scenario_delta_total",
        fmt_opt_f64(row.scenario_delta_total, 1),
    );
    println!(
        "  {:<22} {:>8}",
        "audit_pass_rate",
        fmt_opt_f64(row.audit_pass_rate, 2),
    );
    println!(
        "  {:<22} {:>8}",
        "agents_spawned",
        fmt_opt_i64(row.agents_spawned),
    );
    if let Some(notes) = &row.notes {
        println!("  notes: {}", notes);
    }
}

/// Show one run's health row with threshold flags.
pub fn show(backend: &BackendConnection, date: Option<&str>, json_output: bool) -> Result<()> {
    let conn = sqlite(backend)?;
    let row = match date {
        Some(d) => {
            validate_date(d)?;
            run_health::get_run_health(conn, d)?
        }
        None => run_health::get_latest_run_health(conn)?,
    };

    let Some(row) = row else {
        if json_output {
            println!("{}", json!({ "row": null, "flags": [] }));
        } else {
            match date {
                Some(d) => println!(
                    "No run health recorded for {}. Record with `pftui analytics epistemics record --date {}`.",
                    d, d
                ),
                None => println!(
                    "No run health recorded yet. Record with `pftui analytics epistemics record --date YYYY-MM-DD`."
                ),
            }
        }
        return Ok(());
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "row": row,
                "flags": flags_json(&row),
            }))?
        );
    } else {
        print_row_text(&row);
        let flags = run_health::threshold_flags(&row);
        if flags.is_empty() {
            println!("\n  No epistemic-health flags. Disagreement is alive and well.");
        }
    }
    Ok(())
}

/// Trend table across recorded runs (newest first).
pub fn history(backend: &BackendConnection, limit: Option<usize>, json_output: bool) -> Result<()> {
    let conn = sqlite(backend)?;
    let rows = run_health::list_run_health(conn, limit.unwrap_or(30))?;

    if json_output {
        let payload: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                json!({
                    "row": row,
                    "flags": flags_json(row),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if rows.is_empty() {
        println!("No run health recorded yet.");
    } else {
        println!(
            "{:<12} {:>6} {:>6} {:>6} {:>6} {:>5} {:>7} {:>6} {:>6}  flags",
            "date", "agree", "blind", "panel", "novel", "fbk", "scenΔ", "audit", "agents"
        );
        println!("{}", "-".repeat(86));
        for row in &rows {
            let flags = run_health::threshold_flags(row);
            let flag_str = if flags.is_empty() {
                String::new()
            } else {
                flags
                    .iter()
                    .map(|(m, _)| *m)
                    .collect::<Vec<_>>()
                    .join("⚠ ")
                    + "⚠"
            };
            println!(
                "{:<12} {:>6} {:>6} {:>6} {:>6} {:>5} {:>7} {:>6} {:>6}  {}",
                row.run_date,
                fmt_opt_f64(row.agreement_rate, 2),
                fmt_opt_f64(row.blind_divergence, 2),
                fmt_opt_f64(row.panel_dispersion, 1),
                fmt_opt_f64(row.novelty_rate, 2),
                fmt_opt_i64(row.fallback_warnings),
                fmt_opt_f64(row.scenario_delta_total, 1),
                fmt_opt_f64(row.audit_pass_rate, 2),
                fmt_opt_i64(row.agents_spawned),
                flag_str,
            );
        }
        println!("\n{} run(s)", rows.len());
    }
    Ok(())
}

/// House-vs-rival scoreboard: scored-prediction hit rates by source agent.
pub fn rivalry(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let conn = sqlite(backend)?;
    let report = run_health::compute_rivalry(conn)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let rival_scored = report
        .rows
        .iter()
        .filter(|r| r.camp == "rival")
        .map(|r| r.scored)
        .sum::<i64>();
    if rival_scored == 0 {
        println!(
            "rivalry accruing — antithesis has {} pending prediction(s) and none scored yet.",
            report.antithesis_pending
        );
        if report.rows.is_empty() {
            println!("No scored predictions on any ledger yet.");
            return Ok(());
        }
        println!("House layers so far:\n");
    } else {
        println!("House vs rival — scored-prediction scoreboard\n");
    }

    println!(
        "{:<24} {:<6} {:>7} {:>8} {:>6} {:>8} {:>9}",
        "source_agent", "camp", "scored", "correct", "wrong", "partial", "hit rate"
    );
    println!("{}", "-".repeat(74));
    for r in &report.rows {
        println!(
            "{:<24} {:<6} {:>7} {:>8} {:>6} {:>8} {:>9}",
            r.source_agent,
            r.camp,
            r.scored,
            r.correct,
            r.wrong,
            r.partial,
            r.hit_rate_pct
                .map(|h| format!("{:.1}%", h))
                .unwrap_or_else(|| "—".to_string()),
        );
    }
    if report.antithesis_pending > 0 && rival_scored > 0 {
        println!(
            "\nantithesis still has {} pending prediction(s) accruing.",
            report.antithesis_pending
        );
    }
    Ok(())
}
