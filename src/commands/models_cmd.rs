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
// compare
// ---------------------------------------------------------------------------

/// One model's resolved backtest, carried into the comparison.
pub struct ModelRun {
    pub name: String,
    pub version: u32,
    pub report: PortfolioBacktestReport,
}

/// Run 2+ models over the SAME window + cost assumptions and compare them.
///
/// Each model loads its own price panel over the shared `[from, to]` window
/// (`load_panel` errors loudly if a universe symbol lacks history in-window), so
/// every model is graded on the same calendar. The text output is an aligned
/// table — one row per model plus its three benchmarks — with the best model per
/// metric marked, and a one-line verdict (best risk-adjusted by Calmar).
pub fn run_compare(
    backend: &BackendConnection,
    names: &[String],
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    if names.len() < 2 {
        bail!("compare needs at least 2 models (got {})", names.len());
    }
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
    let mut runs: Vec<ModelRun> = Vec::with_capacity(names.len());
    for name in names {
        let rm = load_resolved(name)?;
        let panel = load_panel(&loader, &rm.model, from_d, to_d)
            .with_context(|| format!("loading price panel for model '{}'", rm.name))?;
        let report = simulate(&rm.model, &panel)
            .with_context(|| format!("simulating model '{}'", rm.name))?;
        runs.push(ModelRun {
            name: rm.name,
            version: rm.version,
            report,
        });
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&compare_json(&runs))?);
        return Ok(());
    }
    print!("{}", render_compare_text(&runs));
    Ok(())
}

/// The metric columns of the comparison, with their "better" direction. `higher`
/// = a larger value is better (CAGR, Sharpe, Sortino, Calmar); otherwise lower is
/// better (MaxDD magnitude, Vol). Time-in-cash / turnover / costs / nRebalances
/// are descriptive (no "best" mark).
struct MetricCol {
    key: &'static str,
    higher_is_better: bool,
}

const RANKED_METRICS: &[MetricCol] = &[
    MetricCol { key: "cagr", higher_is_better: true },
    MetricCol { key: "sharpe", higher_is_better: true },
    MetricCol { key: "sortino", higher_is_better: true },
    MetricCol { key: "maxdd", higher_is_better: false },
    MetricCol { key: "calmar", higher_is_better: true },
    MetricCol { key: "vol", higher_is_better: false },
];

fn metric_value(report: &PortfolioBacktestReport, key: &str) -> f64 {
    let m = &report.metrics;
    match key {
        "cagr" => m.cagr_pct,
        "sharpe" => m.sharpe,
        "sortino" => m.sortino,
        "maxdd" => m.max_drawdown_pct,
        "calmar" => m.calmar,
        "vol" => m.ann_vol_pct,
        _ => f64::NAN,
    }
}

/// Index of the best model run for one ranked metric (None if all NaN). Best is
/// computed across the MODEL rows only — benchmarks are reference lines.
fn best_model_idx(runs: &[ModelRun], col: &MetricCol) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, r) in runs.iter().enumerate() {
        let v = metric_value(&r.report, col.key);
        if v.is_nan() {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, bv)) => {
                if col.higher_is_better {
                    v > bv
                } else {
                    v < bv
                }
            }
        };
        if better {
            best = Some((i, v));
        }
    }
    best.map(|(i, _)| i)
}

fn window_of(runs: &[ModelRun]) -> serde_json::Value {
    // Use the union span across every model's curve (they share the requested
    // window, but trading calendars can differ at the edges).
    let mut from: Option<NaiveDate> = None;
    let mut to: Option<NaiveDate> = None;
    let mut bars = 0usize;
    for r in runs {
        let c = &r.report.daily_equity_curve;
        if let Some(f) = c.first() {
            from = Some(from.map_or(f.date, |x: NaiveDate| x.min(f.date)));
        }
        if let Some(l) = c.last() {
            to = Some(to.map_or(l.date, |x: NaiveDate| x.max(l.date)));
        }
        bars = bars.max(c.len());
    }
    json!({
        "from": from.map(|d| d.to_string()),
        "to": to.map(|d| d.to_string()),
        "bars": bars,
    })
}

fn metrics_block(report: &PortfolioBacktestReport) -> serde_json::Value {
    let m = &report.metrics;
    json!({
        "cagr_pct": m.cagr_pct,
        "sharpe": m.sharpe,
        "sortino": m.sortino,
        "max_drawdown_pct": m.max_drawdown_pct,
        "calmar": m.calmar,
        "ann_vol_pct": m.ann_vol_pct,
        "time_in_cash_pct": m.time_in_cash_pct,
        "avg_turnover_pct_per_yr": m.avg_turnover_pct_per_yr,
        "total_costs": report.total_costs,
        "n_rebalances": report.n_rebalances,
    })
}

fn bench_metrics_block(b: &BenchmarkResult) -> serde_json::Value {
    let m = &b.metrics;
    json!({
        "cagr_pct": m.cagr_pct,
        "sharpe": m.sharpe,
        "sortino": m.sortino,
        "max_drawdown_pct": m.max_drawdown_pct,
        "calmar": m.calmar,
        "ann_vol_pct": m.ann_vol_pct,
        "time_in_cash_pct": m.time_in_cash_pct,
        "avg_turnover_pct_per_yr": m.avg_turnover_pct_per_yr,
        "total_costs": m.total_costs,
    })
}

/// Structured comparison: `{ window, best, verdict, models: [...] }`.
pub fn compare_json(runs: &[ModelRun]) -> serde_json::Value {
    let mut best = serde_json::Map::new();
    for col in RANKED_METRICS {
        if let Some(i) = best_model_idx(runs, col) {
            best.insert(col.key.to_string(), json!(runs[i].name));
        }
    }
    let models: Vec<_> = runs
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "version": r.version,
                "metrics": metrics_block(&r.report),
                "benchmarks": {
                    "static_base_policy": bench_metrics_block(&r.report.benchmarks.static_base_policy),
                    "rebalanced_base_policy": bench_metrics_block(&r.report.benchmarks.rebalanced_base_policy),
                    "equal_weight": bench_metrics_block(&r.report.benchmarks.equal_weight),
                },
            })
        })
        .collect();
    json!({
        "command": "analytics models compare",
        "window": window_of(runs),
        "best": best,
        "verdict": verdict_line(runs),
        "models": models,
    })
}

/// "best risk-adjusted: <model> by Calmar (<v>)" — or a neutral note if nothing
/// ranks. Calmar is the headline risk-adjusted metric (CAGR per unit max DD).
fn verdict_line(runs: &[ModelRun]) -> String {
    let calmar = RANKED_METRICS.iter().find(|c| c.key == "calmar").unwrap();
    match best_model_idx(runs, calmar) {
        Some(i) => format!(
            "best risk-adjusted: {} by Calmar ({:.2})",
            runs[i].name,
            metric_value(&runs[i].report, "calmar")
        ),
        None => "no rankable result (degenerate curves)".to_string(),
    }
}

/// The aligned comparison table (returned as a string so it's unit-testable).
pub fn render_compare_text(runs: &[ModelRun]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let w = window_of(runs);
    let from = w["from"].as_str().unwrap_or("?");
    let to = w["to"].as_str().unwrap_or("?");
    let _ = writeln!(
        out,
        "Positioning model comparison — {} models over {} → {}",
        runs.len(),
        from,
        to
    );
    let _ = writeln!(out);

    // Header. Model/benchmark label + the ranked metrics + descriptive columns.
    let _ = writeln!(
        out,
        "{:<28} {:>8} {:>7} {:>7} {:>8} {:>7} {:>7} {:>7} {:>8} {:>10} {:>5}",
        "MODEL / benchmark",
        "CAGR%",
        "Sharpe",
        "Sortino",
        "MaxDD%",
        "Calmar",
        "Vol%",
        "Cash%",
        "Turn/yr",
        "Costs",
        "nReb",
    );

    // Best model index per ranked metric (for the `*` mark on model rows).
    let best: Vec<Option<usize>> = RANKED_METRICS
        .iter()
        .map(|c| best_model_idx(runs, c))
        .collect();

    for (i, r) in runs.iter().enumerate() {
        let m = &r.report.metrics;
        // Marked cells for the ranked metrics (append '*' when this row is best).
        let mark = |col_i: usize, v: f64, prec: usize| -> String {
            let star = best[col_i] == Some(i);
            let s = format!("{v:.prec$}");
            if star {
                format!("{s}*")
            } else {
                s
            }
        };
        let _ = writeln!(
            out,
            "{:<28} {:>8} {:>7} {:>7} {:>8} {:>7} {:>7} {:>7.1} {:>8.1} {:>10} {:>5}",
            truncate(&r.name, 28),
            mark(0, m.cagr_pct, 2),
            mark(1, m.sharpe, 2),
            mark(2, m.sortino, 2),
            mark(3, m.max_drawdown_pct, 2),
            mark(4, m.calmar, 2),
            mark(5, m.ann_vol_pct, 2),
            m.time_in_cash_pct,
            m.avg_turnover_pct_per_yr,
            r.report.total_costs.round_dp(2).to_string(),
            r.report.n_rebalances,
        );
        // Benchmark rows (no best-mark; nRebalances not tracked per benchmark).
        let b = &r.report.benchmarks;
        bench_row(&mut out, "  static base policy", &b.static_base_policy);
        bench_row(&mut out, "  rebalanced base policy", &b.rebalanced_base_policy);
        bench_row(&mut out, "  equal weight", &b.equal_weight);
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", verdict_line(runs));
    let _ = writeln!(out, "(* = best across models for that metric)");
    out
}

fn bench_row(out: &mut String, label: &str, b: &BenchmarkResult) {
    use std::fmt::Write as _;
    let m = &b.metrics;
    let _ = writeln!(
        out,
        "{:<28} {:>8.2} {:>7.2} {:>7.2} {:>8.2} {:>7.2} {:>7.2} {:>7.1} {:>8.1} {:>10} {:>5}",
        label,
        m.cagr_pct,
        m.sharpe,
        m.sortino,
        m.max_drawdown_pct,
        m.calmar,
        m.ann_vol_pct,
        m.time_in_cash_pct,
        m.avg_turnover_pct_per_yr,
        m.total_costs.round_dp(2).to_string(),
        "-",
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
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

// ---------------------------------------------------------------------------
// tests — compare logic over SYNTHETIC in-memory panels (no DB, no real money)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::portfolio_sim::{
        AssetSpec, CashYield, ClassTarget, FillMode, PortfolioModel, PricePanel, RebalanceBandMode,
        RebalanceCadence, WithinClass,
    };
    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal_macros::dec;

    /// A single-growth-asset model rebalanced weekly to 80% growth / 20% cash.
    fn growth_model(symbol: &str) -> PortfolioModel {
        PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![AssetSpec::new(symbol, "growth")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.2), dec!(0), dec!(1)),
                ClassTarget::new("growth", dec!(0.8), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0.001),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
            no_average_down: false,
        }
    }

    /// 70 consecutive daily closes starting at 100, compounding at `daily_pct`/day.
    fn ramp_series(daily_pct: f64) -> Vec<(NaiveDate, Decimal)> {
        let mut out = Vec::new();
        let mut px = 100.0_f64;
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        for i in 0..70 {
            let d = start + chrono::Duration::days(i);
            out.push((d, Decimal::from_f64(px).unwrap().round_dp(4)));
            px *= 1.0 + daily_pct;
        }
        out
    }

    fn run(symbol: &str, daily_pct: f64) -> ModelRun {
        let model = growth_model(symbol);
        let mut panel = PricePanel::new();
        panel.insert_series(symbol, ramp_series(daily_pct));
        let report = super::simulate(&model, &panel).unwrap();
        ModelRun {
            name: format!("m-{symbol}"),
            version: 1,
            report,
        }
    }

    #[test]
    fn compare_json_has_a_row_per_model_and_marks_the_best() {
        // FAST ramps harder than SLOW → FAST should win CAGR/Calmar.
        let runs = vec![run("FAST", 0.004), run("SLOW", 0.001)];

        let v = compare_json(&runs);
        // one row per model
        let models = v["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["name"], "m-FAST");
        assert_eq!(models[1]["name"], "m-SLOW");
        // each row carries its three benchmarks
        for m in models {
            let b = &m["benchmarks"];
            assert!(b["static_base_policy"]["cagr_pct"].is_number());
            assert!(b["rebalanced_base_policy"]["cagr_pct"].is_number());
            assert!(b["equal_weight"]["cagr_pct"].is_number());
        }
        // best CAGR + Calmar both belong to the faster model
        assert_eq!(v["best"]["cagr"], "m-FAST");
        assert_eq!(v["best"]["calmar"], "m-FAST");
        // verdict names the risk-adjusted winner
        assert!(v["verdict"].as_str().unwrap().contains("m-FAST"));
        // window present
        assert!(v["window"]["from"].is_string());
        assert!(v["window"]["bars"].as_u64().unwrap() > 0);
    }

    #[test]
    fn compare_text_table_marks_winner_and_lists_benchmarks() {
        let runs = vec![run("FAST", 0.004), run("SLOW", 0.001)];
        let txt = render_compare_text(&runs);
        // header + both model rows + their benchmark rows
        assert!(txt.contains("MODEL / benchmark"));
        assert!(txt.contains("m-FAST"));
        assert!(txt.contains("m-SLOW"));
        assert!(txt.contains("static base policy"));
        assert!(txt.contains("rebalanced base policy"));
        assert!(txt.contains("equal weight"));
        // at least one best-mark star, and the verdict + legend
        assert!(txt.contains('*'));
        assert!(txt.contains("best risk-adjusted"));
        assert!(txt.contains("(* = best across models"));
    }

    #[test]
    fn best_direction_lower_is_better_for_drawdown_and_vol() {
        // Equal CAGR target but FAST is more volatile via a deeper interim dip is
        // hard to force deterministically; instead assert the directional helper
        // picks the lower value for a lower-is-better column.
        let runs = vec![run("FAST", 0.004), run("SLOW", 0.001)];
        let vol_col = RANKED_METRICS.iter().find(|c| c.key == "vol").unwrap();
        let idx = best_model_idx(&runs, vol_col).unwrap();
        let v0 = metric_value(&runs[0].report, "vol");
        let v1 = metric_value(&runs[1].report, "vol");
        let expected = if v0 <= v1 { 0 } else { 1 };
        assert_eq!(idx, expected, "lower vol must win the vol column");
    }
}
