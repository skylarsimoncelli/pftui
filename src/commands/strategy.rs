//! CLI handlers for `pftui analytics strategy {backtest|segment|compare|explain}`.
//!
//! Wires the database (`price_history`) to the pure strategy engine in
//! `analytics::strategy` via a [`PriceHistoryLoader`], then renders human and
//! `--json` output.

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::analytics::strategy::{
    self,
    eval::{self, Val},
    parser::{self, PriceField, Timeframe},
    resolver::{resolve_alias, Resolver, SeriesLoader},
};
use crate::db::backend::BackendConnection;

/// Loads full oldest-first series from `price_history` for any symbol/field.
struct PriceHistoryLoader<'a> {
    conn: &'a Connection,
}

impl SeriesLoader for PriceHistoryLoader<'_> {
    fn load(&self, symbol: &str, field: PriceField) -> Result<Vec<(String, f64)>> {
        let col = match field {
            PriceField::Close => "close",
            PriceField::Open => "open",
            PriceField::High => "high",
            PriceField::Low => "low",
            PriceField::Volume => "volume",
        };
        let sql = format!(
            "SELECT date, {col} FROM price_history WHERE symbol = ?1 AND {col} IS NOT NULL ORDER BY date ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([symbol], |row| {
            let date: String = row.get(0)?;
            let raw: String = row.get(1)?;
            Ok((date, raw))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (date, raw) = r?;
            if let Ok(v) = raw.parse::<f64>() {
                out.push((date, v));
            }
        }
        Ok(out)
    }
}

/// Build a resolver over the primary asset, applying the optional [from, to]
/// window to the master axis (indicators still warm up on full history).
fn build_resolver<'a>(
    loader: &'a PriceHistoryLoader<'a>,
    asset: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(Resolver<'a>, String)> {
    let primary = resolve_alias(asset);
    let full = loader.load(&primary, PriceField::Close)?;
    if full.is_empty() {
        bail!(
            "no price history for '{asset}' (resolved to '{primary}'). Fetch it first with `pftui data refresh` or check the symbol/alias."
        );
    }
    let master: Vec<String> = full
        .into_iter()
        .map(|(d, _)| d)
        .filter(|d| from.is_none_or(|f| d.as_str() >= f))
        .filter(|d| to.is_none_or(|t| d.as_str() <= t))
        .collect();
    if master.is_empty() {
        bail!("no bars in the requested date window");
    }
    Ok((Resolver::new(master, &primary, loader), primary))
}

#[allow(clippy::too_many_arguments)]
pub fn run_backtest(
    backend: &BackendConnection,
    asset: &str,
    entry: &str,
    exit: Option<&str>,
    stop_loss: Option<f64>,
    take_profit: Option<f64>,
    trailing_stop: Option<f64>,
    commission: Option<f64>,
    slippage: Option<f64>,
    next_bar_fill: bool,
    vol_target: Option<f64>,
    vol_window: usize,
    max_leverage: f64,
    from: Option<&str>,
    to: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    // Validate the risk parameters (percents must be positive; stops < 100%).
    for (name, v) in [
        ("stop-loss", stop_loss),
        ("take-profit", take_profit),
        ("trailing-stop", trailing_stop),
    ] {
        if let Some(p) = v {
            if p <= 0.0 {
                bail!("--{name} must be a positive percent (got {p})");
            }
        }
    }
    for (name, v) in [("stop-loss", stop_loss), ("trailing-stop", trailing_stop)] {
        if let Some(p) = v {
            if p >= 100.0 {
                bail!("--{name} must be below 100% (got {p})");
            }
        }
    }
    // Costs must be non-negative and sane (a >100% per-side cost is a typo).
    for (name, v) in [("commission", commission), ("slippage", slippage)] {
        if let Some(p) = v {
            if p < 0.0 {
                bail!("--{name} must be non-negative (got {p})");
            }
            if p >= 100.0 {
                bail!("--{name} is a per-side PERCENT; {p} looks like a typo (use 0.1 for 0.1%)");
            }
        }
    }

    let loader = PriceHistoryLoader {
        conn: backend.sqlite(),
    };
    let entry_expr = parser::parse(entry)?;
    let exit_spec = strategy::parse_exit(exit)?;
    let risk = strategy::RiskExits {
        stop_loss_pct: stop_loss,
        take_profit_pct: take_profit,
        trailing_pct: trailing_stop,
    };
    let cost = strategy::Costs {
        commission_pct: commission.unwrap_or(0.0),
        slippage_pct: slippage.unwrap_or(0.0),
        fill_delay_bars: if next_bar_fill { 1 } else { 0 },
    };
    // Vol-target sizing (opt-in). Validate the knobs.
    let sizing = if let Some(vt) = vol_target {
        if vt <= 0.0 {
            bail!("--vol-target must be a positive annualized percent (got {vt})");
        }
        if vol_window < 2 {
            bail!("--vol-window must be at least 2 bars (got {vol_window})");
        }
        if max_leverage <= 0.0 {
            bail!("--max-leverage must be positive (got {max_leverage})");
        }
        Some(strategy::SizingConfig {
            vol_target_pct: vt,
            vol_window,
            max_leverage,
        })
    } else {
        None
    };
    let (mut resolver, primary) = build_resolver(&loader, asset, from, to)?;
    let report =
        strategy::run_backtest(&mut resolver, &entry_expr, &exit_spec, risk, cost, sizing)?;
    let missing = resolver.missing_symbols();
    if !missing.is_empty() {
        bail!(
            "referenced symbol(s) resolved to NO price history: {} — likely a typo, or a ticker with ^/=/- that must use its alias (gold, silver, us10y, fedfunds, dxy, vix). Validate with `analytics strategy explain`.",
            missing.join(", ")
        );
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "strategy backtest",
                "asset": asset,
                "resolved_symbol": primary,
                "entry": entry,
                "exit": exit.unwrap_or("hold 90d (default)"),
                "costs": {
                    "commission_pct_per_side": cost.commission_pct,
                    "slippage_pct_per_side": cost.slippage_pct,
                    "fill": if next_bar_fill { "next-bar-close" } else { "same-bar-close" },
                },
                "report": report,
            }))?
        );
        return Ok(());
    }

    println!("═══ Strategy Backtest: {} ({}) ═══", asset, primary);
    println!("Entry: {entry}");
    println!("Exit:  {}", exit.unwrap_or("hold 90d (default)"));
    if commission.is_some() || slippage.is_some() || next_bar_fill {
        println!(
            "Costs: {:.3}%/side commission · {:.3}%/side slippage · {} fill",
            cost.commission_pct,
            cost.slippage_pct,
            if next_bar_fill { "next-bar" } else { "same-bar" },
        );
    }
    let b = &report.benchmark_hold;
    println!(
        "Window: {} → {} ({:.1}y)",
        b.first_date, b.last_date, b.years
    );
    println!();
    if report.n_trades == 0 {
        if report.n_open_skipped > 0 {
            println!(
                "{} position(s) opened but never closed within the data window — no completed trades to score. \
                 The entry fired; the exit rule never triggered (try a shorter `hold Nd` or a different exit).",
                report.n_open_skipped
            );
        } else {
            println!("No trades triggered — the entry condition never fired. Try `strategy explain` to check the expression resolves over this window.");
        }
        return Ok(());
    }
    println!(
        "Trades: {}  |  Win rate: {}  |  Avg hold: {}d",
        report.n_trades,
        fmt_pct(report.win_rate_pct),
        report
            .avg_days_held
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "—".into()),
    );
    println!(
        "Per-trade: mean {} | median {} | best {} | worst {}",
        fmt_pct(report.mean_return_pct),
        fmt_pct(report.median_return_pct),
        fmt_pct(report.best_return_pct),
        fmt_pct(report.worst_return_pct),
    );
    println!(
        "Strategy:  total {:+.1}%  | CAGR {} | maxDD {:.1}% | time-in-mkt {:.0}%",
        report.total_return_pct,
        fmt_pct(report.cagr_pct),
        report.max_drawdown_pct,
        report.time_in_market_pct,
    );
    println!(
        "Buy & hold: total {:+.1}% | CAGR {} | maxDD {:.1}%",
        b.total_return_pct,
        fmt_pct(b.cagr_pct),
        b.max_drawdown_pct,
    );
    let fmt_ratio = |o: Option<f64>| o.map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".into());
    println!(
        "Tearsheet: profit-factor {} | expectancy {} | payoff {} | Sortino {} | Calmar {} | max-consec-loss {}",
        fmt_ratio(report.profit_factor),
        fmt_pct(report.expectancy_pct),
        fmt_ratio(report.payoff_ratio),
        fmt_ratio(report.sortino_ratio),
        fmt_ratio(report.calmar_ratio),
        report.max_consecutive_losses,
    );
    println!(
        "           avg win {} | avg loss {}",
        fmt_pct(report.avg_win_pct),
        fmt_pct(report.avg_loss_pct),
    );
    if let Some(d) = &report.drawdown_metrics {
        // Drawdown-path risk: the tail of the DRAWDOWN distribution (vs Calmar's
        // single worst point) + duration-aware Ulcer + distribution-shape Omega.
        println!(
            "Drawdown:  Ulcer {:.1}% | Martin {} | CDaR-90 {:.1}% | CDaR-95 {:.1}% | Omega(τ=0) {}",
            d.ulcer_index_pct,
            fmt_ratio(d.martin_ratio),
            d.cdar_90 * 100.0,
            d.cdar_95 * 100.0,
            fmt_ratio(d.omega_ratio),
        );
    }
    if !report.exit_reason_counts.is_empty()
        && report.exit_reason_counts.keys().any(|k| k != "rule")
    {
        let mix: Vec<String> = report
            .exit_reason_counts
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        println!("Exits:     {}", mix.join(" | "));
    }
    if report.n_open_skipped > 0 {
        println!("(+{} open position not yet closed, excluded)", report.n_open_skipped);
    }
    if let Some(v) = &report.validation {
        if v.anecdotal {
            println!(
                "Honesty:   n={} trades — too few for a reliable edge estimate (anecdotal). \
                 Treat any pattern here as a lead, not evidence.",
                report.n_trades
            );
        } else {
            let psr = v
                .psr_vs_zero
                .map(|d| format!("{:.0}%", d * 100.0))
                .unwrap_or_else(|| "n/a (degenerate distribution)".into());
            let ci = v
                .mean_return_ci_pct
                .map(|(lo, hi)| format!("[{lo:+.1}%, {hi:+.1}%]"))
                .unwrap_or_else(|| "—".into());
            println!(
                "Honesty:   edge>0 (PSR) {} | mean-return 90% CI {} | dispersion ratio {} \
                 (single-rule PSR, not deflated; mixed holding periods)",
                psr,
                ci,
                v.trade_dispersion_ratio
                    .map(|s| format!("{s:.2}"))
                    .unwrap_or_else(|| "—".into()),
            );
        }
        if stop_loss.is_some() || take_profit.is_some() || trailing_stop.is_some() {
            println!(
                "           ⚠ risk exits bound each trade's outcome, which compresses the return dispersion — the PSR/CI can look more consistent than the underlying edge."
            );
        }
    }
    if let Some(mc) = &report.monte_carlo {
        println!(
            "Monte-Carlo: {} paths — terminal {:+.0}% / {:+.0}% / {:+.0}% (unlucky/median/lucky) | drawdown: typical {:.0}%, 1-in-20 path {:.0}%, 1-in-100 path {:.0}% | P(loss) {:.0}%",
            mc.n_paths,
            mc.terminal_return_p5_pct,
            mc.terminal_return_p50_pct,
            mc.terminal_return_p95_pct,
            mc.drawdown_median_pct,
            mc.drawdown_p95_pct,
            mc.drawdown_p99_pct,
            mc.prob_loss_pct,
        );
    }
    if let Some(s) = &report.sizing {
        println!(
            "Sizing:    vol-target {:.0}% (≤{:.1}× lev) — sized total {:+.1}% | CAGR {} | maxDD {:.1}% | Sortino {} | leverage avg {:.2}× (range {:.2}–{:.2}×)",
            s.vol_target_pct,
            s.max_leverage,
            s.sized_total_return_pct,
            fmt_pct(s.sized_cagr_pct),
            s.sized_max_drawdown_pct,
            s.sized_sortino_ratio.map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".into()),
            s.avg_leverage,
            s.min_leverage,
            s.max_leverage_used,
        );
        if s.n_neutral_fallback > 0 {
            println!(
                "           ⚠ {} trade(s) had unknown entry-bar vol (warmup/gap) → sized at a neutral 1× (placeholder, not a measured weight).",
                s.n_neutral_fallback
            );
        }
    }
    println!();
    let show = limit.unwrap_or(20).min(report.trades.len());
    println!("Last {show} trades:");
    println!("{:<12} {:<12} {:>10} {:>6}", "Entry", "Exit", "Return", "Days");
    for t in report.trades.iter().rev().take(show).rev() {
        println!(
            "{:<12} {:<12} {:>9.1}% {:>6}",
            t.entry_date, t.exit_date, t.return_pct, t.days_held
        );
    }
    Ok(())
}

/// Parameter sweep: substitute each `--values` entry for the `$P` placeholder in
/// the entry rule, backtest each, and apply the Deflated Sharpe Ratio across the
/// grid — so the BEST config is judged AFTER accounting for selection over N
/// trials (the overfitting guard that a single backtest can't give).
#[allow(clippy::too_many_arguments)]
pub fn run_sweep(
    backend: &BackendConnection,
    asset: &str,
    entry: &str,
    values: &str,
    exit: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    use crate::research::validation as v;
    if !entry.contains("$P") {
        bail!("--entry must contain the sweep placeholder `$P` (e.g. \"rsi(14) < $P\")");
    }
    let vals: Vec<String> = values
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if vals.len() < 2 {
        bail!("--values needs at least 2 comma-separated values to sweep");
    }
    // Validate values are numeric up front so a typo errors clearly instead of
    // failing late as a "no price history" symbol-resolution error.
    for v in &vals {
        if v.parse::<f64>().is_err() {
            bail!("--values entry '{v}' is not a number");
        }
    }

    let loader = PriceHistoryLoader { conn: backend.sqlite() };
    let exit_spec = strategy::parse_exit(exit)?;
    let (mut resolver, primary) = build_resolver(&loader, asset, from, to)?;

    struct Cfg {
        value: String,
        n_trades: usize,
        win_rate: Option<f64>,
        mean_pct: Option<f64>,
        profit_factor: Option<f64>,
        sharpe: Option<f64>,
        rets: Vec<f64>,
    }
    let mut cfgs: Vec<Cfg> = Vec::new();
    for value in &vals {
        let entry_str = entry.replace("$P", value);
        let expr = parser::parse(&entry_str)
            .map_err(|e| anyhow::anyhow!("value '{value}' → parse error: {e}"))?;
        let report = strategy::run_backtest(
            &mut resolver,
            &expr,
            &exit_spec,
            strategy::RiskExits::default(),
            strategy::Costs::default(),
            None,
        )?;
        let rets: Vec<f64> = report.trades.iter().map(|t| t.return_pct / 100.0).collect();
        let sharpe = v::sharpe(&rets);
        cfgs.push(Cfg {
            value: value.clone(),
            n_trades: report.n_trades,
            win_rate: report.win_rate_pct,
            mean_pct: report.mean_return_pct,
            profit_factor: report.profit_factor,
            sharpe,
            rets,
        });
    }
    let missing = resolver.missing_symbols();
    if !missing.is_empty() {
        bail!(
            "referenced symbol(s) resolved to NO price history: {} — likely a typo or a ticker needing its alias.",
            missing.join(", ")
        );
    }

    // Trial Sharpes — EVERY swept value counts as a trial (a config you ran is
    // a trial you ran). A no-trade/degenerate config contributes 0 (it had no
    // edge); dropping such configs would silently shrink n_trials AND collapse
    // the trial-Sharpe variance, LOOSENING the Deflated-Sharpe bar — which an
    // operator could exploit by padding --values with never-firing entries.
    let trial_sharpes: Vec<f64> = cfgs.iter().map(|c| c.sharpe.unwrap_or(0.0)).collect();
    let best = cfgs
        .iter()
        .filter(|c| c.n_trades >= 10 && c.sharpe.is_some())
        .max_by(|a, b| a.sharpe.partial_cmp(&b.sharpe).unwrap());
    let deflated = best.and_then(|b| v::deflated_sharpe_ratio(&b.rets, &trial_sharpes));

    if json_output {
        let rows: Vec<_> = cfgs
            .iter()
            .map(|c| {
                json!({
                    "value": c.value,
                    "n_trades": c.n_trades,
                    "win_rate_pct": c.win_rate,
                    "mean_return_pct": c.mean_pct,
                    "profit_factor": c.profit_factor,
                    "per_trade_sharpe": c.sharpe,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "strategy sweep",
                "asset": asset,
                "resolved_symbol": primary,
                "entry": entry,
                "exit": exit.unwrap_or("hold 90d (default)"),
                "values": vals,
                "configs": rows,
                "best_value": best.map(|b| b.value.clone()),
                "deflated_sharpe": deflated,
            }))?
        );
        return Ok(());
    }

    println!("═══ Strategy Sweep: {} ({}) ═══", asset, primary);
    println!("Entry: {entry}   (sweeping $P over {} values)", vals.len());
    println!("Exit:  {}\n", exit.unwrap_or("hold 90d (default)"));
    println!("{:<10} {:>7} {:>8} {:>9} {:>8} {:>9}", "$P", "trades", "win%", "mean%", "PF", "Sharpe");
    println!("{}", "─".repeat(56));
    for c in &cfgs {
        let best_mark = if best.map(|b| b.value.as_str()) == Some(c.value.as_str()) { " ◀ best" } else { "" };
        println!(
            "{:<10} {:>7} {:>8} {:>9} {:>8} {:>9}{}",
            c.value,
            c.n_trades,
            fmt_pct(c.win_rate),
            fmt_pct(c.mean_pct),
            c.profit_factor.map(|p| format!("{p:.2}")).unwrap_or_else(|| "—".into()),
            c.sharpe.map(|s| format!("{s:.3}")).unwrap_or_else(|| "—".into()),
            best_mark,
        );
    }
    println!();
    match deflated {
        Some(d) => {
            println!(
                "Multiple-testing: best per-trade Sharpe {:.3} across {} trials → \
                 expected-max-by-luck {:.3}, Deflated Sharpe (P real) {:.0}% → {}",
                d.sharpe,
                d.n_trials,
                d.expected_max_sharpe,
                d.dsr * 100.0,
                if d.passes {
                    "the best config's edge SURVIVES selection over the grid"
                } else {
                    "this PARAMETER SELECTION is NOT proven after the search — the best value isn't distinguishable from the luckiest of the grid (not a claim the underlying rule is bad)"
                },
            );
        }
        None => println!("Multiple-testing: no config had ≥10 trades with a usable return spread — sweep is anecdotal."),
    }
    println!("(In-sample best is optimistic by construction; the Deflated Sharpe is the honest read. Per-trade Sharpe mixes holding periods — not a time-based Sharpe.)");
    Ok(())
}

/// Walk-forward optimization: split the timeline into folds, optimize `$P` on
/// each train segment, and measure the chosen value on the NEXT (held-out) test
/// segment. The Walk-Forward Efficiency (OOS edge / in-sample-best edge) is the
/// honest "does the optimization generalize, or is it curve-fit?" read.
///
/// Warmup-correct: each param is backtested over FULL history once, then trades
/// are partitioned by entry date into segments — so indicators warm up on all
/// data rather than losing their lookback at each window boundary.
#[allow(clippy::too_many_arguments)]
pub fn run_walkforward(
    backend: &BackendConnection,
    asset: &str,
    entry: &str,
    values: &str,
    exit: Option<&str>,
    folds: usize,
    json_output: bool,
) -> Result<()> {
    use crate::research::validation as v;
    if !entry.contains("$P") {
        bail!("--entry must contain the sweep placeholder `$P` (e.g. \"rsi(14) < $P\")");
    }
    if folds < 2 {
        bail!("--folds must be at least 2");
    }
    let vals: Vec<String> = values
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if vals.len() < 2 {
        bail!("--values needs at least 2 comma-separated values to optimize over");
    }

    let loader = PriceHistoryLoader { conn: backend.sqlite() };
    let exit_spec = strategy::parse_exit(exit)?;
    // Full-history resolver (no window) so indicators warm up on all data; we
    // partition the resulting TRADES by date into folds afterward.
    let (mut resolver, primary) = build_resolver(&loader, asset, None, None)?;
    let master: Vec<String> = resolver.master_dates().to_vec();
    if master.len() < folds * 30 {
        bail!("not enough history for {folds} folds (need ≥{} bars, have {})", folds * 30, master.len());
    }

    // Backtest each param over full history → its trades (entry_date, return).
    let mut by_param: Vec<(String, Vec<(String, f64)>)> = Vec::new();
    for value in &vals {
        let entry_str = entry.replace("$P", value);
        let expr = parser::parse(&entry_str)
            .map_err(|e| anyhow::anyhow!("value '{value}' → parse error: {e}"))?;
        let report = strategy::run_backtest(
            &mut resolver,
            &expr,
            &exit_spec,
            strategy::RiskExits::default(),
            strategy::Costs::default(),
            None,
        )?;
        let trades: Vec<(String, f64)> = report
            .trades
            .iter()
            .map(|t| (t.entry_date.clone(), t.return_pct / 100.0))
            .collect();
        by_param.push((value.clone(), trades));
    }
    let missing = resolver.missing_symbols();
    if !missing.is_empty() {
        bail!("referenced symbol(s) resolved to NO price history: {}", missing.join(", "));
    }

    // Split the master axis into `folds + 1` contiguous segments by index; fold
    // i optimizes on segment i, tests on segment i+1.
    let n = master.len();
    let seg = |k: usize| -> (String, String) {
        let lo = k * n / (folds + 1);
        let hi = ((k + 1) * n / (folds + 1)).saturating_sub(1).max(lo);
        (master[lo].clone(), master[hi].clone())
    };
    let sharpe_in = |trades: &[(String, f64)], from: &str, to: &str| -> (usize, Option<f64>, Option<f64>) {
        let rets: Vec<f64> = trades
            .iter()
            .filter(|(d, _)| d.as_str() >= from && d.as_str() <= to)
            .map(|(_, r)| *r)
            .collect();
        let mean = (!rets.is_empty()).then(|| rets.iter().sum::<f64>() / rets.len() as f64);
        (rets.len(), mean, v::sharpe(&rets))
    };

    struct Fold {
        train: (String, String),
        test: (String, String),
        best_value: Option<String>,
        is_sharpe: Option<f64>,
        is_n: usize,
        oos_sharpe: Option<f64>,
        oos_mean_pct: Option<f64>,
        oos_n: usize,
    }
    let mut fold_rows: Vec<Fold> = Vec::new();
    for i in 0..folds {
        let train = seg(i);
        let test = seg(i + 1);
        // Optimize: best param by in-sample Sharpe (require ≥5 train trades).
        let mut best: Option<(String, f64)> = None;
        for (val, trades) in &by_param {
            let (cnt, _m, sh) = sharpe_in(trades, &train.0, &train.1);
            if cnt >= 5 {
                if let Some(s) = sh {
                    if best.as_ref().map(|(_, bs)| s > *bs).unwrap_or(true) {
                        best = Some((val.clone(), s));
                    }
                }
            }
        }
        let (oos_n, oos_mean, oos_sharpe, best_value, is_sharpe, is_n) = match &best {
            Some((val, is_s)) => {
                let trades = &by_param.iter().find(|(v, _)| v == val).unwrap().1;
                let (is_cnt, _, _) = sharpe_in(trades, &train.0, &train.1);
                let (cnt, mean, sh) = sharpe_in(trades, &test.0, &test.1);
                (cnt, mean.map(|m| m * 100.0), sh, Some(val.clone()), Some(*is_s), is_cnt)
            }
            None => (0, None, None, None, None, 0),
        };
        fold_rows.push(Fold { train, test, best_value, is_sharpe, is_n, oos_sharpe, oos_mean_pct: oos_mean, oos_n });
    }

    // Aggregate the WFE only over folds with enough OOS trades to be non-noise
    // (a 1–2 trade OOS Sharpe is pure luck and must not dominate). WFE is a
    // ratio-of-averages, only meaningful when the in-sample edge is POSITIVE.
    const MIN_OOS: usize = 5;
    let qual: Vec<&Fold> = fold_rows
        .iter()
        .filter(|f| f.oos_n >= MIN_OOS && f.is_sharpe.is_some() && f.oos_sharpe.is_some())
        .collect();
    let avg_is = (!qual.is_empty()).then(|| qual.iter().filter_map(|f| f.is_sharpe).sum::<f64>() / qual.len() as f64);
    let avg_oos = (!qual.is_empty()).then(|| qual.iter().filter_map(|f| f.oos_sharpe).sum::<f64>() / qual.len() as f64);
    // Only define WFE when the in-sample edge is positive — otherwise the
    // ratio is incoherent (a negative/near-zero denominator flips the sign or
    // inflates it into a meaningless >1 "OOS beats IS" artifact).
    let wfe = match (avg_is, avg_oos) {
        (Some(i), Some(o)) if i > 1e-6 => Some(o / i),
        _ => None,
    };
    let all_same_param = fold_rows
        .iter()
        .filter_map(|f| f.best_value.as_ref())
        .collect::<std::collections::HashSet<_>>()
        .len()
        == 1
        && fold_rows.iter().filter(|f| f.best_value.is_some()).count() > 1;

    if json_output {
        let folds_json: Vec<_> = fold_rows
            .iter()
            .map(|f| {
                json!({
                    "train": [f.train.0, f.train.1],
                    "test": [f.test.0, f.test.1],
                    "best_value": f.best_value,
                    "is_sharpe": f.is_sharpe,
                    "is_trades": f.is_n,
                    "oos_sharpe": f.oos_sharpe,
                    "oos_mean_return_pct": f.oos_mean_pct,
                    "oos_trades": f.oos_n,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "strategy walkforward",
                "asset": asset,
                "resolved_symbol": primary,
                "entry": entry,
                "exit": exit.unwrap_or("hold 90d (default)"),
                "values": vals,
                "folds": folds_json,
                "avg_is_sharpe": avg_is,
                "avg_oos_sharpe": avg_oos,
                "walk_forward_efficiency": wfe,
            }))?
        );
        return Ok(());
    }

    println!("═══ Walk-Forward Optimization: {} ({}) ═══", asset, primary);
    println!("Entry: {entry}   (optimizing $P over {} values, {folds} folds)", vals.len());
    println!("Exit:  {}\n", exit.unwrap_or("hold 90d (default)"));
    let r = |o: Option<f64>| o.map(|x| format!("{x:.3}")).unwrap_or_else(|| "—".into());
    println!("{:<22} {:>8} {:>22} {:>9} {:>8}", "test window", "best $P", "IS Sh (n)", "OOS Sh", "OOS n");
    println!("{}", "─".repeat(74));
    for f in &fold_rows {
        // Flag OOS Sharpes on too few trades — they're noise, excluded from WFE.
        let thin = if f.oos_n > 0 && f.oos_n < MIN_OOS { " *" } else { "" };
        println!(
            "{:<22} {:>8} {:>14} ({:>3}) {:>9} {:>6}{}",
            format!("{}→{}", f.test.0, f.test.1),
            f.best_value.clone().unwrap_or_else(|| "—".into()),
            r(f.is_sharpe),
            f.is_n,
            r(f.oos_sharpe),
            f.oos_n,
            thin,
        );
    }
    if fold_rows.iter().any(|f| f.oos_n > 0 && f.oos_n < MIN_OOS) {
        println!("  (* OOS Sharpe on <{MIN_OOS} trades — noise, excluded from the WFE)");
    }
    println!();
    match wfe {
        Some(w) => {
            let verdict = if w > 1.15 {
                "INCONCLUSIVE — OOS ostensibly beats the in-sample-OPTIMIZED edge, an averaging/small-sample artifact, not a real result"
            } else if w >= 0.5 {
                "ROBUST — OOS retains ≥half the in-sample edge; the optimization generalizes"
            } else {
                "FRAGILE — OOS keeps only a fraction of the in-sample edge; the parameter choice is partly curve-fit"
            };
            println!(
                "Walk-forward efficiency ({} qualifying folds): avg IS Sharpe {} → avg OOS Sharpe {} → WFE {:.2} → {}",
                qual.len(), r(avg_is), r(avg_oos), w, verdict
            );
        }
        None if qual.is_empty() => {
            println!("Walk-forward efficiency: no fold had ≥{MIN_OOS} OOS trades — inconclusive (too thin to judge).");
        }
        None => {
            println!(
                "Walk-forward efficiency: avg in-sample edge ≤0 across the qualifying folds → WFE undefined — the optimizer found no positive in-sample edge to generalize (avg OOS Sharpe {}).",
                r(avg_oos)
            );
        }
    }
    if all_same_param {
        println!("Note: every fold selected the same $P — a stable parameter landscape (or the grid is too coarse to discriminate).");
    }
    println!("(OOS = held-out forward segment never seen during $P selection — the honest forward read. Per-trade Sharpe mixes holding periods — not annualized.)");
    Ok(())
}

pub fn run_segment(
    backend: &BackendConnection,
    asset: &str,
    when: &str,
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let loader = PriceHistoryLoader {
        conn: backend.sqlite(),
    };
    let when_expr = parser::parse(when)?;
    let (mut resolver, primary) = build_resolver(&loader, asset, from, to)?;
    let report = strategy::run_segment(&mut resolver, &when_expr)?;
    let missing = resolver.missing_symbols();
    if !missing.is_empty() {
        bail!(
            "referenced symbol(s) resolved to NO price history: {} — likely a typo, or a ticker with ^/=/- that must use its alias (gold, silver, us10y, fedfunds, dxy, vix). Validate with `analytics strategy explain`.",
            missing.join(", ")
        );
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "strategy segment",
                "asset": asset,
                "resolved_symbol": primary,
                "when": when,
                "report": report,
            }))?
        );
        return Ok(());
    }
    println!("═══ Regime Segmentation: {} ({}) ═══", asset, primary);
    println!("In-state when: {when}");
    println!();
    print_segment_row("IN-STATE", &report.on);
    print_segment_row("OUT", &report.off);
    println!(
        "Buy & hold:   total {:+.1}% over {:.1}y",
        report.benchmark_hold.total_return_pct, report.benchmark_hold.years
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_compare(
    backend: &BackendConnection,
    asset: &str,
    when: &str,
    when_label: &str,
    vs: &str,
    vs_label: &str,
    from: Option<&str>,
    to: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let loader = PriceHistoryLoader {
        conn: backend.sqlite(),
    };
    let when_expr = parser::parse(when)?;
    let vs_expr = parser::parse(vs)?;
    let (mut resolver, primary) = build_resolver(&loader, asset, from, to)?;
    let report = strategy::run_compare(&mut resolver, &when_expr, when_label, &vs_expr, vs_label)?;
    let missing = resolver.missing_symbols();
    if !missing.is_empty() {
        bail!(
            "referenced symbol(s) resolved to NO price history: {} — likely a typo, or a ticker with ^/=/- that must use its alias (gold, silver, us10y, fedfunds, dxy, vix). Validate with `analytics strategy explain`.",
            missing.join(", ")
        );
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "strategy compare",
                "asset": asset,
                "resolved_symbol": primary,
                "when": { "label": when_label, "expr": when },
                "vs": { "label": vs_label, "expr": vs },
                "report": report,
            }))?
        );
        return Ok(());
    }
    println!("═══ Regime Comparison: {} ({}) ═══", asset, primary);
    println!("{when_label}: {when}");
    println!("{vs_label}: {vs}");
    println!();
    print_segment_row(when_label, &report.a);
    print_segment_row(vs_label, &report.b);
    println!(
        "Buy & hold:   total {:+.1}% over {:.1}y",
        report.benchmark_hold.total_return_pct, report.benchmark_hold.years
    );
    Ok(())
}

pub fn run_explain(
    backend: &BackendConnection,
    asset: &str,
    entry: &str,
    json_output: bool,
) -> Result<()> {
    let loader = PriceHistoryLoader {
        conn: backend.sqlite(),
    };
    let expr = parser::parse(entry)?;
    let (mut resolver, primary) = build_resolver(&loader, asset, None, None)?;
    let val = eval::eval(&expr, Timeframe::Daily, &mut resolver)?;
    // Surface typo'd / historyless symbols — explain is the recommended
    // validation tool, so it must catch the same missing-symbol case the
    // backtest/segment/compare handlers bail on (here we report rather than
    // bail, since explain's job is to diagnose).
    let missing = resolver.missing_symbols();

    let (kind, n_known, first, last, firings) = match &val {
        Val::Num(s) => {
            let (n, f, l) = coverage_num(s, resolver.master_dates());
            ("numeric", n, f, l, None)
        }
        Val::Bool(s) => {
            let (n, f, l) = coverage_bool(s, resolver.master_dates());
            let fires = s.iter().filter(|x| **x == Some(true)).count();
            ("boolean", n, f, l, Some(fires))
        }
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "strategy explain",
                "asset": asset,
                "resolved_symbol": primary,
                "expr": entry,
                "value_kind": kind,
                "master_bars": resolver.master_len(),
                "resolved_bars": n_known,
                "first_resolved_date": first,
                "last_resolved_date": last,
                "firings": firings,
                "missing_symbols": missing,
            }))?
        );
        return Ok(());
    }
    println!("Expression parsed OK.");
    println!("  Asset:        {asset} → {primary}");
    println!("  Value kind:   {kind}");
    if !missing.is_empty() {
        println!(
            "  ⚠ MISSING:    referenced symbol(s) with NO price history: {} — likely a typo, or a ticker with ^/=/- that must use its alias (gold, silver, us10y, fedfunds, dxy, vix).",
            missing.join(", ")
        );
    }
    println!(
        "  Coverage:     {n_known}/{} bars resolved ({} → {})",
        resolver.master_len(),
        first.as_deref().unwrap_or("—"),
        last.as_deref().unwrap_or("—"),
    );
    if let Some(f) = firings {
        println!("  Firings:      {f} bars where the condition is true");
    } else {
        println!("  Note:         numeric series — use a comparison/crossing to form a condition");
    }
    Ok(())
}

fn coverage_num(s: &[Option<f64>], dates: &[String]) -> (usize, Option<String>, Option<String>) {
    let known: Vec<usize> = (0..s.len()).filter(|&i| s[i].is_some()).collect();
    coverage(&known, dates)
}
fn coverage_bool(s: &[Option<bool>], dates: &[String]) -> (usize, Option<String>, Option<String>) {
    let known: Vec<usize> = (0..s.len()).filter(|&i| s[i].is_some()).collect();
    coverage(&known, dates)
}
fn coverage(known: &[usize], dates: &[String]) -> (usize, Option<String>, Option<String>) {
    let first = known.first().and_then(|&i| dates.get(i).cloned());
    let last = known.last().and_then(|&i| dates.get(i).cloned());
    (known.len(), first, last)
}

fn fmt_pct(v: Option<f64>) -> String {
    v.map(|x| format!("{x:+.1}%")).unwrap_or_else(|| "—".into())
}

fn print_segment_row(label: &str, s: &strategy::engine::SegmentStats) {
    println!(
        "{:<12} {:>5} days ({:>4.0}% of bars, {:>3} episodes) | mean/day {} | annualized {} | up-days {}",
        label,
        s.n_days,
        s.share_of_days_pct,
        s.episodes,
        fmt_pct(s.mean_daily_return_pct),
        fmt_pct(s.annualized_return_pct),
        fmt_pct(s.up_day_share_pct),
    );
}
