//! CLI handlers for `pftui analytics models {list|show|backtest}`.
//!
//! The "operable bridge" stage (POSITIONING-MODELS.md §3.5, P2-CLI): wires the
//! TOML spec parser ([`analytics::portfolio_sim::spec`]) and the price-panel
//! loader ([`analytics::portfolio_sim::loader`]) to the P0/P1 in-memory
//! simulator, and renders the [`PortfolioBacktestReport`] for humans + `--json`.
//!
//! NO SQLite storage yet (the `positioning_models` catalog + run cache land in
//! the next stage). Specs are discovered from a `models/` directory in the
//! current working directory; a `<name|path>` argument is either a bare model
//! name (resolved to `models/<name>.toml`) or a direct path to a `.toml` file.
//!
//! Rules are PARSED but NOT evaluated here (the signal-rule engine is P3): a
//! model that declares rules runs as its `base_policy` with a loud warning.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::json;

use crate::analytics::portfolio_sim::engine::{simulate, BenchmarkResult, PortfolioBacktestReport};
use crate::analytics::portfolio_sim::loader::{load_panel, CloseSeriesLoader};
use crate::analytics::portfolio_sim::spec::{resolve_str, ResolvedModel};
use crate::db::backend::BackendConnection;

/// Directory (relative to cwd) scanned for `models/*.toml` specs.
const MODELS_DIR: &str = "models";

// ---------------------------------------------------------------------------
// DB-backed close loader (price_history) — market closes ONLY, never portfolio
// dollars. Mirrors `commands::strategy::PriceHistoryLoader`.
// ---------------------------------------------------------------------------

struct PriceHistoryCloseLoader<'a> {
    conn: &'a rusqlite::Connection,
}

impl CloseSeriesLoader for PriceHistoryCloseLoader<'_> {
    fn load_closes(&self, symbol: &str) -> Result<Vec<(NaiveDate, Decimal)>> {
        let mut stmt = self.conn.prepare(
            "SELECT date, close FROM price_history WHERE symbol = ?1 AND close IS NOT NULL ORDER BY date ASC",
        )?;
        let rows = stmt.query_map([symbol], |row| {
            let date: String = row.get(0)?;
            let close: String = row.get(1)?;
            Ok((date, close))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (date, close) = r?;
            let (Ok(d), Ok(c)) = (
                NaiveDate::parse_from_str(&date, "%Y-%m-%d"),
                close.parse::<Decimal>(),
            ) else {
                continue;
            };
            out.push((d, c));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Spec discovery / resolution.
// ---------------------------------------------------------------------------

/// Resolve a `<name|path>` argument to a spec file path: a value ending in
/// `.toml` or containing a path separator is treated as a direct path;
/// otherwise it's a bare model name resolved to `models/<name>.toml`.
fn spec_path(name_or_path: &str) -> PathBuf {
    if name_or_path.ends_with(".toml") || name_or_path.contains('/') {
        PathBuf::from(name_or_path)
    } else {
        Path::new(MODELS_DIR).join(format!("{name_or_path}.toml"))
    }
}

/// Load + resolve a spec by name or path.
fn load_resolved(name_or_path: &str) -> Result<ResolvedModel> {
    let path = spec_path(name_or_path);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read model spec '{}'", path.display()))?;
    resolve_str(&text).with_context(|| format!("invalid model spec '{}'", path.display()))
}

/// Scan `models/` for `*.toml` specs, returning sorted paths.
fn discover_specs() -> Result<Vec<PathBuf>> {
    let dir = Path::new(MODELS_DIR);
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("could not read models directory '{}'", dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

pub fn run_list(json_output: bool) -> Result<()> {
    let paths = discover_specs()?;

    if json_output {
        let items: Vec<_> = paths
            .iter()
            .map(|path| {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                match std::fs::read_to_string(path)
                    .map_err(anyhow::Error::from)
                    .and_then(|t| resolve_str(&t))
                {
                    Ok(rm) => json!({
                        "name": rm.name,
                        "file": name,
                        "path": path.to_string_lossy(),
                        "version": rm.version,
                        "universe_size": rm.model.universe.len(),
                        "rules": rm.rules.len(),
                        "valid": true,
                    }),
                    Err(e) => json!({
                        "name": name,
                        "file": name,
                        "path": path.to_string_lossy(),
                        "valid": false,
                        "error": e.to_string(),
                    }),
                }
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "analytics models list",
                "models_dir": MODELS_DIR,
                "count": items.len(),
                "models": items,
            }))?
        );
        return Ok(());
    }

    if paths.is_empty() {
        println!("No model specs found in ./{MODELS_DIR}/ (add a *.toml spec there).");
        return Ok(());
    }
    println!("Model specs in ./{MODELS_DIR}/:\n");
    println!("{:<24} {:>3} {:>8} {:>6}  FILE", "NAME", "VER", "UNIVERSE", "RULES");
    for path in &paths {
        let file = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match std::fs::read_to_string(path)
            .map_err(anyhow::Error::from)
            .and_then(|t| resolve_str(&t))
        {
            Ok(rm) => println!(
                "{:<24} {:>3} {:>8} {:>6}  {}.toml",
                rm.name,
                rm.version,
                rm.model.universe.len(),
                rm.rules.len(),
                file
            ),
            Err(e) => {
                println!("{file:<24}   -        -      -  {file}.toml  [INVALID: {e}]")
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

pub fn run_show(name_or_path: &str, json_output: bool) -> Result<()> {
    let rm = load_resolved(name_or_path)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&resolved_to_json(&rm))?);
        return Ok(());
    }

    println!("Model: {} (v{})", rm.name, rm.version);
    println!("Base currency: {}   Initial capital: {}", rm.model.base_currency, rm.model.initial_capital);
    println!(
        "Cadence: {:?}   Band: {:?}   Fill: {:?}",
        rm.model.rebalance_cadence, rm.model.rebalance_band_mode, rm.model.fill
    );
    println!(
        "Commission: {}   Slippage: {}   Cash yield: {:?}",
        rm.model.commission_pct, rm.model.slippage_pct, rm.model.cash_yield
    );
    if let Some(mp) = rm.max_position {
        println!("Max position (advisory, not yet enforced): {mp}");
    }
    println!("\nUniverse ({}):", rm.model.universe.len());
    for a in &rm.model.universe {
        println!("  {:<10} class={:<12} ccy={}", a.symbol, a.class, a.price_currency);
    }
    println!("\nClass targets:");
    println!("  {:<14} {:>8} {:>8} {:>8}", "CLASS", "TARGET", "FLOOR", "CEILING");
    for t in &rm.model.targets {
        println!("  {:<14} {:>8} {:>8} {:>8}", t.class, t.target, t.floor, t.ceiling);
    }
    println!("\nRules: {} (parsed, NOT evaluated in this stage — signal-rule engine lands in P3)", rm.rules.len());
    for r in &rm.rules {
        println!("  [{}] priority={} when: {}", r.id, r.priority, r.when);
    }
    if !rm.params.is_empty() {
        println!("\nParams:");
        for (k, v) in &rm.params {
            println!("  {k} = {v}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// backtest
// ---------------------------------------------------------------------------

pub fn run_backtest(
    backend: &BackendConnection,
    name_or_path: &str,
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let rm = load_resolved(name_or_path)?;
    let from_d = parse_date_opt(from, "--from")?;
    let to_d = parse_date_opt(to, "--to")?;
    if let (Some(f), Some(t)) = (from_d, to_d) {
        if f > t {
            bail!("--from ({f}) is after --to ({t})");
        }
    }

    let loader = PriceHistoryCloseLoader {
        conn: backend.sqlite(),
    };
    let panel = load_panel(&loader, &rm.model, from_d, to_d)?;
    let report = simulate(&rm.model, &panel)?;

    // Warnings: any deferred/dropped legs, infeasible rebalances. (Rules now
    // evaluate as real signal rules — see the engine; a bad rule fails at
    // resolve time rather than running silently as base_policy.)
    let mut warnings: Vec<String> = Vec::new();
    let deferred: usize = report.rebalance_events.iter().map(|e| e.deferred_legs.len()).sum();
    let dropped: usize = report.rebalance_events.iter().map(|e| e.dropped_legs.len()).sum();
    if deferred > 0 {
        warnings.push(format!("{deferred} leg(s) deferred (non-tradable on a rebalance date)"));
    }
    if dropped > 0 {
        warnings.push(format!("{dropped} leg(s) dropped (no future close to fill against)"));
    }
    let n_infeasible = report.rebalance_events.iter().filter(|e| e.infeasible).count();
    if n_infeasible > 0 {
        warnings.push(format!("{n_infeasible} rebalance(s) flagged infeasible (held prior weights)"));
    }

    if json_output {
        let mut payload = json!({
            "command": "analytics models backtest",
            "model": resolved_to_json(&rm),
            "window": window_json(&report),
            "warnings": warnings,
            "report": report,
        });
        // hoist a compact headline so consumers don't have to dig through `report`.
        payload["headline"] = headline_json(&report);
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    render_text(&rm, &report, &warnings);
    Ok(())
}

// ---------------------------------------------------------------------------
// rendering helpers
// ---------------------------------------------------------------------------

fn render_text(rm: &ResolvedModel, report: &PortfolioBacktestReport, warnings: &[String]) {
    let curve = &report.daily_equity_curve;
    let (first, last) = match (curve.first(), curve.last()) {
        (Some(f), Some(l)) => (f, l),
        _ => {
            println!("(empty backtest — no daily curve)");
            return;
        }
    };
    let m = &report.metrics;
    println!("Positioning backtest: {} (v{})", rm.name, rm.version);
    println!(
        "Window: {} → {}  ({} bars, {} rebalances)",
        first.date, last.date, curve.len(), report.n_rebalances
    );
    println!();
    println!("  Final equity        {}", last.equity.round_dp(2));
    println!("  CAGR                {:>8.2}%", m.cagr_pct);
    println!("  Max drawdown        {:>8.2}%", m.max_drawdown_pct);
    println!("  Ann. volatility     {:>8.2}%", m.ann_vol_pct);
    println!("  Sharpe              {:>8.2}", m.sharpe);
    println!("  Sortino             {:>8.2}", m.sortino);
    println!("  Calmar              {:>8.2}", m.calmar);
    println!("  CDaR-95             {:>8.2}%", m.cdar_95_pct);
    println!("  Ulcer index         {:>8.2}%", m.ulcer_index_pct);
    println!("  Time in cash        {:>8.2}%", m.time_in_cash_pct);
    println!("  Avg turnover/yr     {:>8.2}%", m.avg_turnover_pct_per_yr);
    println!("  Total costs         {}", report.total_costs.round_dp(2));
    println!();
    println!("  Benchmarks                       CAGR     MaxDD");
    print_bench("    static base policy", &report.benchmarks.static_base_policy);
    print_bench("    rebalanced base policy", &report.benchmarks.rebalanced_base_policy);
    print_bench("    equal weight", &report.benchmarks.equal_weight);

    if !warnings.is_empty() {
        println!();
        for w in warnings {
            println!("  WARNING: {w}");
        }
    }
}

fn print_bench(label: &str, b: &BenchmarkResult) {
    println!(
        "{label:<32} {:>7.2}% {:>7.2}%",
        b.metrics.cagr_pct, b.metrics.max_drawdown_pct
    );
}

fn resolved_to_json(rm: &ResolvedModel) -> serde_json::Value {
    json!({
        "name": rm.name,
        "version": rm.version,
        "base_currency": rm.model.base_currency,
        "initial_capital": rm.model.initial_capital,
        "cash_class": rm.model.cash_class,
        "rebalance_cadence": format!("{:?}", rm.model.rebalance_cadence),
        "rebalance_band_mode": format!("{:?}", rm.model.rebalance_band_mode),
        "fill": format!("{:?}", rm.model.fill),
        "commission_pct": rm.model.commission_pct,
        "slippage_pct": rm.model.slippage_pct,
        "cash_yield": format!("{:?}", rm.model.cash_yield),
        "max_position": rm.max_position,
        "no_average_down": rm.no_average_down,
        "universe": rm.model.universe.iter().map(|a| json!({
            "symbol": a.symbol, "class": a.class, "price_currency": a.price_currency,
        })).collect::<Vec<_>>(),
        "targets": rm.model.targets.iter().map(|t| json!({
            "class": t.class, "target": t.target, "floor": t.floor, "ceiling": t.ceiling,
        })).collect::<Vec<_>>(),
        "rules": rm.rules,
        "rules_evaluated": false,
        "params": rm.params,
    })
}

fn window_json(report: &PortfolioBacktestReport) -> serde_json::Value {
    let curve = &report.daily_equity_curve;
    json!({
        "from": curve.first().map(|p| p.date.to_string()),
        "to": curve.last().map(|p| p.date.to_string()),
        "bars": curve.len(),
    })
}

fn headline_json(report: &PortfolioBacktestReport) -> serde_json::Value {
    let final_equity = report
        .daily_equity_curve
        .last()
        .map(|p| p.equity)
        .unwrap_or_default();
    json!({
        "final_equity": final_equity,
        "cagr_pct": report.metrics.cagr_pct,
        "max_drawdown_pct": report.metrics.max_drawdown_pct,
        "sharpe": report.metrics.sharpe,
        "n_rebalances": report.n_rebalances,
        "total_costs": report.total_costs,
        "bench_static_cagr_pct": report.benchmarks.static_base_policy.metrics.cagr_pct,
        "bench_rebalanced_cagr_pct": report.benchmarks.rebalanced_base_policy.metrics.cagr_pct,
        "bench_equal_weight_cagr_pct": report.benchmarks.equal_weight.metrics.cagr_pct,
    })
}

fn parse_date_opt(s: Option<&str>, flag: &str) -> Result<Option<NaiveDate>> {
    match s {
        None => Ok(None),
        Some(v) => NaiveDate::parse_from_str(v, "%Y-%m-%d")
            .map(Some)
            .with_context(|| format!("{flag} must be a YYYY-MM-DD date (got '{v}')")),
    }
}
