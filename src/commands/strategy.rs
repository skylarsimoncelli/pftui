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
    let (mut resolver, primary) = build_resolver(&loader, asset, from, to)?;
    let report = strategy::run_backtest(&mut resolver, &entry_expr, &exit_spec, risk, cost)?;
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
