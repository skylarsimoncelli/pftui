//! `pftui research` — CLI surface of the research harness (R1a).
//!
//! - `research signals list` — the registry (id, version, description)
//! - `research backtest` — event studies for signals × assets; persists
//!   `signal_expectancy` rows (L2, rebuildable) and prints the table
//! - `research expectancy` — read the persisted table (latest as_of)
//! - `research events` — the raw dated event list with per-event forward
//!   returns ("show me the 12 instances")

use anyhow::{bail, Context, Result};
use rust_decimal::Decimal;
use serde_json::json;
use std::collections::HashMap;

use crate::db::backend::BackendConnection;
use crate::db::signal_expectancy::{self, ExpectancyRow};
use crate::models::asset::AssetCategory;
use crate::models::price::HistoryRecord;
use crate::models::transaction::TxType;
use crate::research::event_study::{self, EventStudy};
use crate::research::registry::{self, AssetContext, SignalEmitter};

/// Minimum daily bars for an honest event study on an asset.
const MIN_BARS: usize = 250;

/// Full-depth history fetch (the deep series for GC=F/SI=F reaches back to
/// ~2000, BTC-USD to 2014 — `technicals_structure::load_deep_history` caps
/// at 2600 rows, which would silently halve the event sample).
const FULL_HISTORY_LIMIT: u32 = 20_000;

/// Same deep-series substitution as `technicals_structure::load_deep_history`
/// (shallow held alias like BTC falls back to the deeper `BTC-USD` series),
/// but with full historical depth — event studies need the whole sample.
pub fn load_deep_history_full(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<(String, Vec<HistoryRecord>)> {
    let sym = symbol.to_uppercase();
    let primary = crate::db::price_history::get_history_backend(backend, &sym, FULL_HISTORY_LIMIT)?;
    if primary.len() >= 400 || sym.contains('-') || sym.contains('=') {
        return Ok((sym, primary));
    }
    let alt_sym = format!("{sym}-USD");
    let alt = crate::db::price_history::get_history_backend(backend, &alt_sym, FULL_HISTORY_LIMIT)?;
    if alt.len() > primary.len() {
        Ok((alt_sym, alt))
    } else {
        Ok((sym, primary))
    }
}

pub fn run_signals_list(json_output: bool) -> Result<()> {
    let defs = registry::registry();
    if json_output {
        let rows: Vec<_> = defs
            .iter()
            .map(|d| {
                json!({
                    "id": d.id(),
                    "version": d.version(),
                    "description": d.description(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("Signal registry ({} signals)", defs.len());
    println!("{:<34} {:>3}  description", "id", "ver");
    for d in defs {
        println!("{:<34} {:>3}  {}", d.id(), d.version(), d.description());
    }
    Ok(())
}

pub fn run_backtest(
    backend: &BackendConnection,
    signal: Option<&str>,
    asset: Option<&str>,
    as_of: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = backend
        .sqlite_native()
        .context("research backtest requires the SQLite backend")?;
    let as_of = match as_of {
        Some(d) => {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .with_context(|| format!("invalid --as-of date '{d}' (expected YYYY-MM-DD)"))?;
            d.to_string()
        }
        None => chrono::Local::now().format("%Y-%m-%d").to_string(),
    };
    let defs = resolve_signals(signal)?;
    let assets = resolve_assets(backend, asset)?;
    if assets.is_empty() {
        bail!("no assets to backtest — no held positions found and no --asset given");
    }

    let mut all_rows: Vec<ExpectancyRow> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for sym in &assets {
        let Some((series, history)) = load_series(backend, sym, &as_of)? else {
            skipped.push(format!("{sym} (insufficient history)"));
            continue;
        };
        let Some(ctx) = AssetContext::build(sym, &series, &history) else {
            skipped.push(format!("{sym} (context build failed)"));
            continue;
        };
        for def in &defs {
            let events = def.emit(&ctx);
            let study = event_study::study(&ctx.dates, &ctx.closes, &ctx.sma200, &events, &as_of);
            for h in &study.horizons {
                if h.n_total == 0 {
                    continue; // signal never fired for this asset — no row
                }
                all_rows.push(ExpectancyRow {
                    signal_id: def.id().to_string(),
                    signal_version: def.version().to_string(),
                    asset: ctx.series.clone(),
                    horizon_days: h.horizon_days,
                    as_of: as_of.clone(),
                    n_total: h.n_total as i64,
                    n_evaluable: h.n_evaluable as i64,
                    n_nonoverlap: h.n_nonoverlap as i64,
                    hit_rate: h.hit_rate,
                    baseline_hit_rate: h.baseline_hit_rate,
                    hit_lift: h.hit_lift,
                    mean_pct: h.mean_pct,
                    baseline_mean_pct: h.baseline_mean_pct,
                    mean_lift: h.mean_lift,
                    median_pct: h.median_pct,
                    p25: h.p25,
                    p75: h.p75,
                    mae_mean: h.mae_mean,
                    mae_worst: h.mae_worst,
                    mfe_mean: h.mfe_mean,
                    p_value: h.p_value,
                    significant: h.significant_5pct,
                    computed_at: None,
                });
            }
        }
    }

    signal_expectancy::upsert_rows(conn, &all_rows)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "as_of": as_of,
                "assets": assets,
                "signals": defs.iter().map(|d| d.id()).collect::<Vec<_>>(),
                "rows_persisted": all_rows.len(),
                "skipped": skipped,
                "rows": all_rows,
            }))?
        );
        return Ok(());
    }

    println!(
        "Research backtest — as_of {as_of} | {} signals x {} assets | {} rows persisted",
        defs.len(),
        assets.len(),
        all_rows.len()
    );
    if !skipped.is_empty() {
        println!("skipped: {}", skipped.join(", "));
    }
    print_expectancy_table(&all_rows);
    Ok(())
}

pub fn run_expectancy(
    backend: &BackendConnection,
    signal: Option<&str>,
    asset: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = backend
        .sqlite_native()
        .context("research expectancy requires the SQLite backend")?;
    // Map a held alias (BTC) onto the persisted deep series (BTC-USD).
    let asset_series = asset.map(|a| {
        load_deep_history_full(backend, a)
            .map(|(series, _)| series)
            .unwrap_or_else(|_| a.to_uppercase())
    });
    if let Some(s) = signal {
        if registry::find_signal(s).is_none() {
            bail!("unknown signal '{s}' — see `pftui research signals list`");
        }
    }
    let rows = signal_expectancy::latest_rows(conn, signal, asset_series.as_deref())?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No persisted expectancy rows — run `pftui research backtest` first.");
        return Ok(());
    }
    println!("Signal expectancy (latest as_of per signal x asset)");
    print_expectancy_table(&rows);
    Ok(())
}

pub fn run_events(
    backend: &BackendConnection,
    signal: &str,
    asset: &str,
    limit: usize,
    json_output: bool,
) -> Result<()> {
    let Some(def) = registry::find_signal(signal) else {
        bail!("unknown signal '{signal}' — see `pftui research signals list`");
    };
    let (series, history) = load_deep_history_full(backend, asset)?;
    if history.len() < MIN_BARS {
        bail!(
            "insufficient history for {} ({} daily rows; need >= {MIN_BARS})",
            asset.to_uppercase(),
            history.len()
        );
    }
    let as_of = history
        .last()
        .map(|r| r.date.clone())
        .unwrap_or_default();
    let Some(ctx) = AssetContext::build(asset, &series, &history) else {
        bail!("could not build the research context for {}", asset.to_uppercase());
    };
    let events = def.emit(&ctx);
    let study = event_study::study(&ctx.dates, &ctx.closes, &ctx.sma200, &events, &as_of);
    let shown = tail_events(&study, limit);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "signal": def.id(),
                "version": def.version(),
                "description": def.description(),
                "asset": ctx.series,
                "as_of": as_of,
                "n_events_total": study.events.len(),
                "horizons": study.horizons,
                "events": shown,
            }))?
        );
        return Ok(());
    }

    println!(
        "{} v{} on {} — {} events (showing last {})",
        def.id(),
        def.version(),
        ctx.series,
        study.events.len(),
        shown.len()
    );
    println!("{}", def.description());
    println!();
    for e in shown {
        let outcomes: Vec<String> = e
            .outcomes
            .iter()
            .map(|o| {
                let tag = format!("{}d", o.horizon_days);
                match o.return_pct {
                    Some(r) if o.kept => format!("{tag} {r:+.1}%"),
                    Some(r) => format!("{tag} {r:+.1}% (overlap)"),
                    None => format!("{tag} —"),
                }
            })
            .collect();
        println!("{}  {}", e.date, outcomes.join("  "));
        println!("    {}", e.detail);
    }
    println!();
    println!("Stats (overlap-pruned, vs baseline drift):");
    for h in &study.horizons {
        println!("  {}", format_stats_line(h));
    }
    Ok(())
}

/// Event study for one (signal, asset) at the series' latest date. `None`
/// when the signal is unknown, the history is too shallow, or the research
/// context can't build. Reused by the competence dossier's worked-precedent
/// section (`research dossier`).
pub fn event_study_for(
    backend: &BackendConnection,
    signal_id: &str,
    asset: &str,
) -> Result<Option<(String, EventStudy)>> {
    let Some(def) = registry::find_signal(signal_id) else {
        return Ok(None);
    };
    let (series, history) = load_deep_history_full(backend, asset)?;
    if history.len() < MIN_BARS {
        return Ok(None);
    }
    let as_of = history.last().map(|r| r.date.clone()).unwrap_or_default();
    let Some(ctx) = AssetContext::build(asset, &series, &history) else {
        return Ok(None);
    };
    let events = def.emit(&ctx);
    let study = event_study::study(&ctx.dates, &ctx.closes, &ctx.sma200, &events, &as_of);
    Ok(Some((ctx.series.clone(), study)))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_signals(signal: Option<&str>) -> Result<Vec<&'static registry::SignalDef>> {
    match signal {
        Some(s) => {
            let Some(def) = registry::find_signal(s) else {
                bail!("unknown signal '{s}' — see `pftui research signals list`");
            };
            Ok(vec![def])
        }
        None => Ok(registry::registry().iter().collect()),
    }
}

/// Default asset set: held positions (net qty > 0, non-cash) + SPY.
fn resolve_assets(backend: &BackendConnection, asset: Option<&str>) -> Result<Vec<String>> {
    if let Some(a) = asset {
        return Ok(vec![a.to_uppercase()]);
    }
    let txs = crate::db::transactions::list_transactions_backend(backend)?;
    let mut qty: HashMap<String, Decimal> = HashMap::new();
    let mut cash: HashMap<String, bool> = HashMap::new();
    for tx in &txs {
        let e = qty.entry(tx.symbol.to_uppercase()).or_default();
        match tx.tx_type {
            TxType::Buy => *e += tx.quantity,
            TxType::Sell => *e -= tx.quantity,
        }
        cash.insert(
            tx.symbol.to_uppercase(),
            tx.category == AssetCategory::Cash,
        );
    }
    let mut symbols: Vec<String> = qty
        .into_iter()
        .filter(|(sym, q)| *q > Decimal::ZERO && !cash.get(sym).copied().unwrap_or(false))
        .map(|(sym, _)| sym)
        .collect();
    symbols.sort();
    if !symbols.iter().any(|s| s == "SPY") {
        symbols.push("SPY".to_string());
    }
    Ok(symbols)
}

/// Deep history truncated to as_of. None when too shallow for a study.
fn load_series(
    backend: &BackendConnection,
    sym: &str,
    as_of: &str,
) -> Result<Option<(String, Vec<HistoryRecord>)>> {
    let (series, history) = load_deep_history_full(backend, sym)?;
    let truncated: Vec<HistoryRecord> = history
        .into_iter()
        .filter(|r| r.date.as_str() <= as_of)
        .collect();
    if truncated.len() < MIN_BARS {
        return Ok(None);
    }
    Ok(Some((series, truncated)))
}

fn tail_events(study: &EventStudy, limit: usize) -> Vec<event_study::EventRow> {
    let start = study.events.len().saturating_sub(limit.max(1));
    study.events[start..].to_vec()
}

fn fmt_opt(v: Option<f64>, suffix: &str) -> String {
    v.map(|x| format!("{x:+.2}{suffix}"))
        .unwrap_or_else(|| "    —".to_string())
}

fn format_stats_line(h: &crate::research::event_study::HorizonStats) -> String {
    let flag = if h.significant_5pct {
        " *sig"
    } else if h.anecdotal {
        " ~anecdotal"
    } else {
        ""
    };
    format!(
        "{:>4}d n={:<3} hit {} (base {}) mean {} (base {}) lift {} mae {} p={}{}",
        h.horizon_days,
        h.n_nonoverlap,
        h.hit_rate
            .map(|x| format!("{:.0}%", x * 100.0))
            .unwrap_or_else(|| "—".into()),
        h.baseline_hit_rate
            .map(|x| format!("{:.0}%", x * 100.0))
            .unwrap_or_else(|| "—".into()),
        fmt_opt(h.mean_pct, "%"),
        fmt_opt(h.baseline_mean_pct, "%"),
        fmt_opt(h.mean_lift, "pp"),
        fmt_opt(h.mae_mean, "%"),
        h.p_value
            .map(|p| format!("{p:.3}"))
            .unwrap_or_else(|| "—".into()),
        flag
    )
}

fn print_expectancy_table(rows: &[ExpectancyRow]) {
    if rows.is_empty() {
        println!("(no signal fired for any asset in scope)");
        return;
    }
    println!(
        "{:<34} {:<9} {:>4} {:>4} {:>11} {:>17} {:>9} {:>8} {:>7}  flags",
        "signal", "asset", "hz", "n", "hit(base)", "mean(base)", "lift", "mae", "p"
    );
    for r in rows {
        let flags = if r.significant {
            "*sig"
        } else if r.n_nonoverlap < event_study::ANECDOTAL_N as i64 {
            "~anec"
        } else {
            ""
        };
        println!(
            "{:<34} {:<9} {:>3}d {:>4} {:>11} {:>17} {:>9} {:>8} {:>7}  {}",
            r.signal_id,
            r.asset,
            r.horizon_days,
            r.n_nonoverlap,
            format!(
                "{}({})",
                r.hit_rate
                    .map(|x| format!("{:.0}%", x * 100.0))
                    .unwrap_or_else(|| "—".into()),
                r.baseline_hit_rate
                    .map(|x| format!("{:.0}%", x * 100.0))
                    .unwrap_or_else(|| "—".into()),
            ),
            format!(
                "{}({})",
                r.mean_pct
                    .map(|x| format!("{x:+.2}%"))
                    .unwrap_or_else(|| "—".into()),
                r.baseline_mean_pct
                    .map(|x| format!("{x:+.2}%"))
                    .unwrap_or_else(|| "—".into()),
            ),
            r.mean_lift
                .map(|x| format!("{x:+.2}pp"))
                .unwrap_or_else(|| "—".into()),
            r.mae_mean
                .map(|x| format!("{x:+.2}%"))
                .unwrap_or_else(|| "—".into()),
            r.p_value
                .map(|p| format!("{p:.3}"))
                .unwrap_or_else(|| "—".into()),
            flags
        );
    }
}
