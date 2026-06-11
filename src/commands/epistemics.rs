//! `pftui analytics epistemics` — run-health instrumentation (epistemics R4).
//!
//! Subcommands:
//!   record           — upsert one run's epistemic-health metrics by date
//!   show             — one run's row with threshold flags
//!   history          — newest-first trend table
//!   rivalry          — house-vs-antithesis scored-prediction scoreboard
//!   conviction-price — per (layer × held asset) conviction-vs-price Pearson r
//!                      (standing rule 15: conviction must not track price)

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

/// Held-asset symbols (quantity > 0) computed from transactions. Used by the
/// conviction-price correlation derivations. Best-effort: an empty portfolio
/// or a non-SQLite transaction store degrades to an empty list.
fn held_asset_symbols(backend: &BackendConnection) -> Vec<String> {
    let txs = crate::db::transactions::list_transactions_backend(backend).unwrap_or_default();
    if txs.is_empty() {
        return Vec::new();
    }
    let empty = std::collections::HashMap::new();
    crate::models::position::compute_positions(&txs, &empty, &empty)
        .into_iter()
        .filter(|p| p.quantity > rust_decimal::Decimal::ZERO)
        .map(|p| p.symbol)
        .collect()
}

/// Default trailing window (days) for the conviction-price correlation when
/// `record` self-derives it.
const CONVICTION_PRICE_DEFAULT_DAYS: i64 = 90;

/// Trailing window (days) for the self-derived forecast hit rate.
const FORECAST_HIT_RATE_WINDOW_DAYS: i64 = 30;

/// Record (upsert) a run's health metrics. Derives what it can when flags
/// are omitted: blind_divergence from same-day analyst views,
/// scenario_delta_total from today's scenario probability ledger,
/// conviction_price_corr (max |r| across canonical layer × held asset pairs
/// over the trailing 90d) from analyst_view_history × price_history,
/// forecast_hit_rate (trailing 30d of forecast_scores), and
/// active_misalignments (current ACTIVE forecast_misalignments count).
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
    conviction_price_corr: Option<f64>,
    forecast_hit_rate: Option<f64>,
    active_misalignments: Option<i64>,
    json_output: bool,
) -> Result<()> {
    let conn = sqlite(backend)?;
    validate_date(date)?;
    validate_unit_interval("--agreement", agreement)?;
    validate_unit_interval("--novelty", novelty)?;
    validate_unit_interval("--audit-pass-rate", audit_pass_rate)?;
    validate_unit_interval("--forecast-hit-rate", forecast_hit_rate)?;

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
    let conviction_price_corr = match conviction_price_corr {
        Some(v) => Some(v),
        None => {
            let held = held_asset_symbols(backend);
            let computed = if held.is_empty() {
                None
            } else {
                let rows = run_health::compute_conviction_price_correlations(
                    conn,
                    &held,
                    CONVICTION_PRICE_DEFAULT_DAYS,
                )?;
                run_health::max_abs_conviction_price_corr(&rows)
            };
            if computed.is_some() {
                derived.push("conviction_price_corr");
            }
            computed
        }
    };
    let forecast_hit_rate = match forecast_hit_rate {
        Some(v) => Some(v),
        None => {
            let computed =
                run_health::compute_forecast_hit_rate(conn, FORECAST_HIT_RATE_WINDOW_DAYS)?;
            if computed.is_some() {
                derived.push("forecast_hit_rate");
            }
            computed
        }
    };
    let active_misalignments = match active_misalignments {
        Some(v) => Some(v),
        None => {
            derived.push("active_misalignments");
            Some(crate::db::forecast_misalignments::count_active(conn)?)
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
        conviction_price_corr,
        forecast_hit_rate,
        active_misalignments,
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
                "active_misalignment_rows": active_misalignment_rows(conn, &row),
            }))?
        );
    } else {
        println!("Recorded run health for {} (id {}).", date, id);
        if !derived.is_empty() {
            println!("  derived by pftui: {}", derived.join(", "));
        }
        print_row_text(&row);
        print_active_misalignments(conn, &row);
    }
    Ok(())
}

/// The ACTIVE misalignment rows backing `row.active_misalignments`, for the
/// "⚠ flag listing them" rendering. Empty unless the count is > 0.
fn active_misalignment_rows(
    conn: &Connection,
    row: &RunHealth,
) -> Vec<crate::db::forecast_misalignments::MisalignmentRow> {
    if row.active_misalignments.unwrap_or(0) <= 0 {
        return Vec::new();
    }
    crate::db::forecast_misalignments::active_misalignments(conn).unwrap_or_default()
}

fn print_active_misalignments(conn: &Connection, row: &RunHealth) {
    let active = active_misalignment_rows(conn, row);
    if active.is_empty() {
        return;
    }
    println!("  active misalignments:");
    for m in &active {
        println!(
            "    ⚠ {}/{} — {} consecutive wrong-sign {} calls ({} → {}, {:+.1}% against)",
            m.layer,
            m.asset,
            m.streak_len,
            m.call,
            m.span_start,
            m.span_end,
            m.cum_realized_against_pct
        );
    }
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
        "  {:<22} {:>8}  {}",
        "conviction_price_corr",
        fmt_opt_f64(row.conviction_price_corr, 2),
        flag_for("conviction_price_corr"),
    );
    println!(
        "  {:<22} {:>8}",
        "forecast_hit_rate",
        fmt_opt_f64(row.forecast_hit_rate, 2),
    );
    println!(
        "  {:<22} {:>8}  {}",
        "active_misalignments",
        fmt_opt_i64(row.active_misalignments),
        flag_for("active_misalignments"),
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

    // Shadow-book benchmark line (R4): the desk-vs-do-nothing scoreboard.
    // Best-effort — a compute failure must never break run-health display.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let shadow_line = crate::commands::shadow_book::summary_line(conn, &today);

    let Some(row) = row else {
        if json_output {
            println!(
                "{}",
                json!({ "row": null, "flags": [], "shadow_book": shadow_line })
            );
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
            if let Some(line) = &shadow_line {
                println!("\n  {line}");
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
                "active_misalignment_rows": active_misalignment_rows(conn, &row),
                "shadow_book": shadow_line,
            }))?
        );
    } else {
        print_row_text(&row);
        print_active_misalignments(conn, &row);
        if let Some(line) = &shadow_line {
            println!("\n  {line}");
        }
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

/// Per (canonical layer × held asset) Pearson correlation between the
/// layer's signed conviction trajectory and the asset's closes (standing
/// rule 15: conviction must not track price). `--asset` overrides the
/// held-asset universe with a single symbol.
pub fn conviction_price(
    backend: &BackendConnection,
    days: i64,
    asset: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = sqlite(backend)?;
    if days <= 0 {
        anyhow::bail!("--days must be positive, got {}", days);
    }
    let assets: Vec<String> = match asset {
        Some(a) => vec![a.to_uppercase()],
        None => held_asset_symbols(backend),
    };
    if assets.is_empty() {
        if json_output {
            println!(
                "{}",
                json!({ "rows": [], "max_abs_r": null, "days": days,
                        "note": "no held assets (and no --asset given)" })
            );
        } else {
            println!("No held assets found (and no --asset given) — nothing to correlate.");
        }
        return Ok(());
    }

    let rows = run_health::compute_conviction_price_correlations(conn, &assets, days)?;
    let max_abs = run_health::max_abs_conviction_price_corr(&rows);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "days": days,
                "rows": rows,
                "max_abs_r": max_abs,
                "flag_threshold": run_health::CONVICTION_PRICE_CORR_CEILING,
                "min_n": run_health::CONVICTION_PRICE_MIN_N,
            }))?
        );
        return Ok(());
    }

    if rows.is_empty() {
        println!(
            "No conviction trajectories found in the last {} day(s) for: {}.",
            days,
            assets.join(", ")
        );
        return Ok(());
    }
    println!(
        "Conviction-price correlation — last {} day(s) (flag: |r| > {:.2})\n",
        days,
        run_health::CONVICTION_PRICE_CORR_CEILING
    );
    println!("{:<8} {:<8} {:>4} {:>8}  flag", "layer", "asset", "n", "r");
    println!("{}", "-".repeat(60));
    for row in &rows {
        let r_str = row
            .r
            .map(|r| format!("{:+.3}", r))
            .unwrap_or_else(|| "insuff.".to_string());
        let flag = if row.flagged {
            "⚠ momentum dressed as structure (standing rule 15)"
        } else {
            ""
        };
        println!(
            "{:<8} {:<8} {:>4} {:>8}  {}",
            row.layer, row.asset, row.n, r_str, flag
        );
    }
    match max_abs {
        Some(m) => println!("\nmax |r| = {:.3} (self-derived into run_health.conviction_price_corr by `epistemics record`)", m),
        None => println!("\nNo pair has ≥{} paired observations yet — correlation accruing.", run_health::CONVICTION_PRICE_MIN_N),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn make_backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        }
    }

    fn seed_lockstep(conn: &Connection, asset: &str, series: &str) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS analyst_view_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL,
                key_evidence TEXT,
                blind_spots TEXT,
                allocation_bias TEXT,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        for i in 0..7i64 {
            let date = (chrono::Utc::now().date_naive() - chrono::Duration::days(7 - i))
                .format("%Y-%m-%d")
                .to_string();
            conn.execute(
                "INSERT INTO analyst_view_history
                    (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
                 VALUES ('medium', ?1, 'bull', ?2, 'r', ?3 || ' 09:00:00')",
                params![asset, i + 1, date],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO price_history (symbol, date, close, source)
                 VALUES (?1, ?2, ?3, 'test')",
                params![series, date, format!("{}", 3000 + i * 50)],
            )
            .unwrap();
        }
    }

    #[test]
    fn record_self_derives_conviction_price_corr_from_held_assets() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        // One held position (buy 1 GC=F) whose conviction tracks price.
        conn.execute(
            "INSERT INTO transactions (symbol, category, tx_type, quantity, price_per, currency, date)
             VALUES ('GC=F', 'commodity', 'buy', '1', '3000', 'USD', '2026-01-05')",
            [],
        )
        .unwrap();
        seed_lockstep(conn, "GC=F", "GC=F");

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        record(
            &backend, &today, Some(0.7), None, None, None, None, None, None, None, None,
            None, // conviction_price_corr omitted → self-derive
            None, // forecast_hit_rate omitted → self-derive
            None, // active_misalignments omitted → self-derive
            true,
        )
        .unwrap();
        let row = run_health::get_run_health(conn, &today).unwrap().unwrap();
        let corr = row
            .conviction_price_corr
            .expect("self-derived conviction_price_corr");
        assert!(corr > 0.95, "lockstep trajectory must derive |r|≈1, got {corr}");
        // And the threshold flag fires through the shared flag path.
        assert!(run_health::threshold_flags(&row)
            .iter()
            .any(|(m, _)| *m == "conviction_price_corr"));
    }

    #[test]
    fn record_explicit_flag_wins_over_derivation() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        record(
            &backend,
            &today,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(0.25),
            None,
            None,
            true,
        )
        .unwrap();
        let row = run_health::get_run_health(conn, &today).unwrap().unwrap();
        assert_eq!(row.conviction_price_corr, Some(0.25));
    }

    #[test]
    fn record_self_derives_forecast_hit_rate_and_active_misalignments() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        // Trailing-30d scored corpus: 1 hit / 4 scored non-neutral cells.
        crate::research::forecast_scoring::ensure_table(conn).unwrap();
        let recent = (chrono::Utc::now().date_naive() - chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();
        for (id, hit) in [(1, true), (2, false), (3, false), (4, false)] {
            conn.execute(
                "INSERT INTO forecast_scores
                    (view_history_id, analyst, asset, direction, conviction, horizon_days,
                     view_date, direction_hit, status)
                 VALUES (?1, 'medium', 'GC=F', 'bull', 3, 45, ?2, ?3, 'scored')",
                rusqlite::params![id, recent, hit as i64],
            )
            .unwrap();
        }
        // One ACTIVE misalignment.
        crate::db::forecast_misalignments::ensure_table(conn).unwrap();
        conn.execute(
            "INSERT INTO forecast_misalignments
                (layer, asset, detected_at, streak_len, call, span_start, span_end,
                 cum_realized_against_pct, status)
             VALUES ('medium', 'GC=F', '2026-06-01 00:00:00', 7, 'bull',
                     '2026-04-01', '2026-04-22', -40.5, 'active')",
            [],
        )
        .unwrap();

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        record(
            &backend, &today, None, None, None, None, None, None, None, None, None, None,
            None, // forecast_hit_rate → self-derive
            None, // active_misalignments → self-derive
            true,
        )
        .unwrap();
        let row = run_health::get_run_health(conn, &today).unwrap().unwrap();
        let rate = row.forecast_hit_rate.expect("self-derived hit rate");
        assert!((rate - 0.25).abs() < 1e-9, "got {rate}");
        assert_eq!(row.active_misalignments, Some(1));
        // The flag fires through the shared threshold path.
        assert!(run_health::threshold_flags(&row)
            .iter()
            .any(|(m, _)| *m == "active_misalignments"));

        // Explicit flags win over derivation.
        record(
            &backend,
            &today,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(0.9),
            Some(0),
            true,
        )
        .unwrap();
        let row = run_health::get_run_health(conn, &today).unwrap().unwrap();
        assert_eq!(row.forecast_hit_rate, Some(0.9));
        assert_eq!(row.active_misalignments, Some(0));
    }

    #[test]
    fn conviction_price_command_runs_on_empty_db() {
        let backend = make_backend();
        conviction_price(&backend, 90, None, true).unwrap();
        conviction_price(&backend, 90, Some("GC=F"), true).unwrap();
        assert!(conviction_price(&backend, 0, None, true).is_err());
    }
}
