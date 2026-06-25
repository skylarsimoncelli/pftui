//! `pftui analytics cycles tracked` — a fast status dashboard over every armed
//! cycle-signal alert (both cycle-BOTTOM and cycle-TOP conditions).
//!
//! For each alert rule whose `condition` is a cycle signal — confluence
//! threshold, single criterion, or single component, in either polarity — this
//! emits one row with:
//!   * asset / polarity / human label / timeframe / target
//!   * armed-at, recurring + cooldown
//!   * fired-yet, last-fired, time-since-last, fire count (from the
//!     triggered-alert log + the rule's own `triggered_at`)
//!   * a CURRENT LIVE READ (met N/7 for confluence, met/unmet for a
//!     criterion/component, plus distance-to-threshold) computed once per
//!     (asset, timeframe, polarity) and reused across rules that share it.
//!
//! It deliberately does NOT run the expensive backtest — this is a status view.
//! All decode/label logic is reused from [`crate::alerts::cycle_signal_alert`]
//! and the live read from [`crate::analytics::cycle_signals`]; nothing is
//! re-invented here. Privacy-safe: signal metadata + counts only, no dollars.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;

use crate::alerts::cycle_signal_alert::{
    self as csa, condition_polarity, criterion_label, parse_condition, top_criterion_label,
    CycleSignalCondition, Polarity,
};
use crate::analytics::cycle_signals::{
    self, CycleBottomSignals, CycleTopSignals, SignalTimeframe,
};
use crate::commands::cli_json;
use crate::commands::cycle_signals_cmd::{load_deep_history, resolve_alias};
use crate::db::backend::BackendConnection;
use crate::db::{alerts, triggered_alerts};
use crate::models::price::HistoryRecord;

/// The cached live-read for one (asset, timeframe, polarity) key. `NoHistory`
/// means the series had zero rows; `Insufficient` means history was too shallow
/// to compute a read (the engine returned `None`). Both render gracefully.
enum CachedRead {
    NoHistory,
    Bottom(Option<CycleBottomSignals>),
    Top(Option<CycleTopSignals>),
}

/// Decoded live state for a single tracked signal row.
#[derive(Debug, Clone, Serialize)]
struct LiveRead {
    /// `true`/`false` when computable; `None` when history was insufficient.
    met: Option<bool>,
    /// Confluence met count on the latest bar (criteria-style rows only).
    #[serde(skip_serializing_if = "Option::is_none")]
    met_count: Option<usize>,
    /// Total composite criteria (always 7) for confluence rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<usize>,
    /// For confluence: how many MORE criteria are needed to reach the target
    /// (0 = already met). For component: the signed distance-to-trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    distance_to_target: Option<f64>,
    /// Human one-liner ("4/7, 1 more to ≥5", "met", "insufficient history").
    summary: String,
}

/// One fully-decoded tracked-signal row.
#[derive(Debug, Clone, Serialize)]
struct TrackedRow {
    alert_id: i64,
    symbol: String,
    asset: String,
    polarity: String,
    /// Signal shape: `confluence` | `criterion` | `component`.
    shape: String,
    label: String,
    timeframe: String,
    target: String,
    status: String,
    armed_at: String,
    recurring: bool,
    cooldown_minutes: i64,
    fired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_fired: Option<String>,
    time_since_last: String,
    fire_count: usize,
    live: LiveRead,
}

/// Aggregate firing history for one alert id, gathered from the triggered-alert
/// log plus the rule's own `triggered_at`.
struct FireHistory {
    count: usize,
    last_fired: Option<String>,
}

pub fn run(
    backend: &BackendConnection,
    asset_filter: Option<&str>,
    polarity_filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    // Parse the polarity filter up front so a typo errors before any work.
    let want_polarity: Option<Polarity> = match polarity_filter.map(|p| p.trim().to_lowercase()) {
        None => None,
        Some(p) if p == "top" => Some(Polarity::Top),
        Some(p) if p == "bottom" => Some(Polarity::Bottom),
        Some(p) => anyhow::bail!("invalid --polarity '{p}' — expected 'top' or 'bottom'"),
    };

    let rules = alerts::list_alerts_backend(backend)?;

    // Index firing history by alert id (one pass over the full triggered log).
    let triggered = triggered_alerts::list_triggered_alerts_backend(backend, None, false)?;
    let mut fires: HashMap<i64, FireHistory> = HashMap::new();
    for t in &triggered {
        let entry = fires.entry(t.alert_id).or_insert(FireHistory {
            count: 0,
            last_fired: None,
        });
        entry.count += 1;
        // The log is ordered DESC, so the first row seen per id is the latest;
        // still take the max defensively in case ordering ever changes.
        match &entry.last_fired {
            Some(prev) if prev.as_str() >= t.triggered_at.as_str() => {}
            _ => entry.last_fired = Some(t.triggered_at.clone()),
        }
    }

    let now = Utc::now();
    let mut read_cache: HashMap<(String, SignalTimeframe, Polarity), CachedRead> = HashMap::new();
    let mut rows: Vec<TrackedRow> = Vec::new();

    for rule in &rules {
        let Some(condition) = rule.condition.as_deref() else {
            continue;
        };
        if !csa::is_cycle_signal_condition(condition) {
            continue;
        }
        let Ok(parsed) = parse_condition(condition) else {
            // Structurally-broken cycle condition — skip rather than panic.
            continue;
        };
        let polarity = condition_polarity(condition).unwrap_or(Polarity::Bottom);
        if let Some(want) = want_polarity {
            if want != polarity {
                continue;
            }
        }
        if let Some(filter) = asset_filter {
            if !symbol_matches(filter, &rule.symbol) {
                continue;
            }
        }

        let timeframe = condition_timeframe(&parsed);
        let live = compute_live_read(
            backend,
            &mut read_cache,
            &rule.symbol,
            timeframe,
            polarity,
            &parsed,
        );

        let fire = fires.get(&rule.id);
        // A rule counts as fired if the log has an event, OR the rule itself
        // carries a triggered_at, OR its status moved off armed.
        let last_fired = newest(fire.and_then(|f| f.last_fired.clone()), rule.triggered_at.clone());
        let fired = fire.map(|f| f.count > 0).unwrap_or(false)
            || rule.triggered_at.is_some()
            || rule.status != crate::alerts::AlertStatus::Armed;
        let fire_count = fire.map(|f| f.count).unwrap_or(0);
        let time_since_last = match &last_fired {
            Some(ts) => humanize_since(ts, now),
            None => "never".to_string(),
        };

        rows.push(TrackedRow {
            alert_id: rule.id,
            symbol: rule.symbol.clone(),
            asset: csa::friendly_asset(&rule.symbol),
            polarity: polarity_label(polarity).to_string(),
            shape: shape_label(&parsed).to_string(),
            label: condition_label(&parsed, polarity),
            timeframe: timeframe.label().to_string(),
            target: target_label(&parsed),
            status: rule.status.to_string(),
            armed_at: rule.created_at.clone(),
            recurring: rule.recurring,
            cooldown_minutes: rule.cooldown_minutes,
            fired,
            last_fired,
            time_since_last,
            fire_count,
            live,
        });
    }

    // Stable, deterministic ordering: asset, then polarity, then id.
    rows.sort_by(|a, b| {
        a.asset
            .cmp(&b.asset)
            .then(a.polarity.cmp(&b.polarity))
            .then(a.alert_id.cmp(&b.alert_id))
    });

    if json_output {
        print_json(&rows);
    } else {
        print_text(&rows, asset_filter, want_polarity);
    }
    Ok(())
}

/// Pull the timeframe out of an already-parsed condition (no string re-parse).
fn condition_timeframe(parsed: &CycleSignalCondition) -> SignalTimeframe {
    match parsed {
        CycleSignalCondition::Confluence { timeframe, .. } => *timeframe,
        CycleSignalCondition::Criterion { timeframe, .. } => *timeframe,
        CycleSignalCondition::Component { timeframe, .. } => *timeframe,
    }
}

fn polarity_label(p: Polarity) -> &'static str {
    match p {
        Polarity::Bottom => "bottom",
        Polarity::Top => "top",
    }
}

fn shape_label(parsed: &CycleSignalCondition) -> &'static str {
    match parsed {
        CycleSignalCondition::Confluence { .. } => "confluence",
        CycleSignalCondition::Criterion { .. } => "criterion",
        CycleSignalCondition::Component { .. } => "component",
    }
}

/// Human label for the condition, reusing the canonical label tables in
/// `cycle_signal_alert` (no label strings are re-invented here).
fn condition_label(parsed: &CycleSignalCondition, polarity: Polarity) -> String {
    match parsed {
        CycleSignalCondition::Confluence { target, .. } => {
            format!("confluence ≥{target}/7")
        }
        CycleSignalCondition::Criterion { criterion_key, .. } => match polarity {
            Polarity::Bottom => criterion_label(criterion_key).to_string(),
            Polarity::Top => top_criterion_label(criterion_key).to_string(),
        },
        CycleSignalCondition::Component { component_key, .. } => match polarity {
            Polarity::Bottom => csa::component_label(component_key).to_string(),
            Polarity::Top => csa::top_component_label(component_key).to_string(),
        },
    }
}

/// Short target descriptor for the table's "target" column.
fn target_label(parsed: &CycleSignalCondition) -> String {
    match parsed {
        CycleSignalCondition::Confluence { target, .. } => format!("≥{target}/7"),
        CycleSignalCondition::Criterion { .. } => "met".to_string(),
        CycleSignalCondition::Component { .. } => "met".to_string(),
    }
}

/// Resolve and cache the live signal read for one (asset, timeframe, polarity),
/// then decode the row-specific state from it. The read is computed at most once
/// per key even when many rules share it.
fn compute_live_read(
    backend: &BackendConnection,
    cache: &mut HashMap<(String, SignalTimeframe, Polarity), CachedRead>,
    symbol: &str,
    timeframe: SignalTimeframe,
    polarity: Polarity,
    parsed: &CycleSignalCondition,
) -> LiveRead {
    let key = (symbol.to_uppercase(), timeframe, polarity);
    let cached = cache
        .entry(key)
        .or_insert_with(|| load_read(backend, symbol, timeframe, polarity));
    decode_live(cached, parsed, polarity)
}

fn load_read(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: SignalTimeframe,
    polarity: Polarity,
) -> CachedRead {
    let history: Vec<HistoryRecord> = match load_deep_history(backend, symbol) {
        Ok((_, h)) => h,
        Err(_) => return CachedRead::NoHistory,
    };
    if history.is_empty() {
        return CachedRead::NoHistory;
    }
    let series = resolve_alias(symbol);
    match polarity {
        Polarity::Bottom => {
            CachedRead::Bottom(cycle_signals::cycle_bottom_signals(&series, &history, timeframe))
        }
        Polarity::Top => {
            CachedRead::Top(cycle_signals::cycle_top_signals(&series, &history, timeframe))
        }
    }
}

fn decode_live(
    cached: &CachedRead,
    parsed: &CycleSignalCondition,
    _polarity: Polarity,
) -> LiveRead {
    match cached {
        CachedRead::NoHistory => LiveRead {
            met: None,
            met_count: None,
            total: None,
            distance_to_target: None,
            summary: "no price history".to_string(),
        },
        CachedRead::Bottom(None) | CachedRead::Top(None) => LiveRead {
            met: None,
            met_count: None,
            total: None,
            distance_to_target: None,
            summary: "insufficient history".to_string(),
        },
        // The MET boolean is taken from the canonical engine path
        // (`evaluate` / `evaluate_top`) so component rows inherit the same
        // `find_component(...).unwrap_or_else(component_fallback)` semantics the
        // alert engine uses — including the fallback-only keys (`erf_positive`,
        // `pi_cycle_bottom`, `erf_negative`, `pi_cycle_top`) that live OUTSIDE
        // `criteria[].components`. We only hand-extract met_count/total + the
        // distance column for display.
        CachedRead::Bottom(Some(sig)) => {
            let met = csa::evaluate(&sig.symbol, parsed, Some(sig)).is_triggered;
            decode_from_parts(
                parsed,
                sig.met_count,
                sig.total,
                met,
                component_distance(&sig.criteria),
            )
        }
        CachedRead::Top(Some(sig)) => {
            let met = csa::evaluate_top(&sig.symbol, parsed, Some(sig)).is_triggered;
            decode_from_parts(
                parsed,
                sig.met_count,
                sig.total,
                met,
                component_distance(&sig.criteria),
            )
        }
    }
}

/// Map of atomic component key → (met, signed distance-to-trigger).
fn component_distance(
    criteria: &[cycle_signals::Criterion],
) -> HashMap<String, (bool, Option<f64>)> {
    let mut out = HashMap::new();
    for c in criteria {
        for comp in &c.components {
            out.entry(comp.key.clone())
                .or_insert((comp.met, comp.distance_to_trigger));
        }
    }
    out
}

/// Decode the display fields for one row. `met` is the AUTHORITATIVE met
/// boolean from the engine path (`evaluate` / `evaluate_top`); met_count/total
/// and the component distance are pulled from the signal struct for context.
fn decode_from_parts(
    parsed: &CycleSignalCondition,
    met_count: usize,
    total: usize,
    met: bool,
    comp: HashMap<String, (bool, Option<f64>)>,
) -> LiveRead {
    match parsed {
        CycleSignalCondition::Confluence { target, .. } => {
            let remaining = target.saturating_sub(met_count);
            let summary = if met {
                format!("met ({met_count}/{total})")
            } else {
                format!("{met_count}/{total}, {remaining} more to ≥{target}")
            };
            LiveRead {
                met: Some(met),
                met_count: Some(met_count),
                total: Some(total),
                distance_to_target: Some(remaining as f64),
                summary,
            }
        }
        CycleSignalCondition::Criterion { .. } => LiveRead {
            met: Some(met),
            met_count: Some(met_count),
            total: Some(total),
            distance_to_target: None,
            summary: format!(
                "{} (suite {met_count}/{total})",
                if met { "met" } else { "not met" }
            ),
        },
        CycleSignalCondition::Component { component_key, .. } => {
            // Distance column comes from the components map (when the key lives
            // there); the met state is the authoritative engine value, which
            // also covers the fallback-only keys absent from the map.
            let dist = comp.get(component_key).and_then(|(_, d)| *d);
            let summary = match dist {
                Some(d) => format!(
                    "{} (dist {:+.2})",
                    if met { "met" } else { "not met" },
                    d
                ),
                None => (if met { "met" } else { "not met" }).to_string(),
            };
            LiveRead {
                met: Some(met),
                met_count: None,
                total: None,
                distance_to_target: dist,
                summary,
            }
        }
    }
}

/// Does the filter token refer to the same asset as `symbol`? Resolves aliases
/// (gold→GC=F) and treats `BTC` / `BTC-USD` as the same base.
fn symbol_matches(filter: &str, symbol: &str) -> bool {
    normalize_sym(filter) == normalize_sym(symbol)
}

fn normalize_sym(s: &str) -> String {
    let resolved = resolve_alias(s).to_uppercase();
    resolved
        .trim_end_matches("-USD")
        .trim_end_matches("USD")
        .to_string()
}

/// Pick the chronologically-newest of two optional timestamps. Compares PARSED
/// instants (not raw strings) so a mix of RFC3339 (`...T..Z`) and SQLite-naive
/// (`YYYY-MM-DD HH:MM:SS`) shapes sorts correctly — lexical compare would
/// mis-rank them ('T' > ' '). Falls back to lexical only if a value won't parse.
fn newest(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => {
            let later = match (parse_ts(&x), parse_ts(&y)) {
                (Some(px), Some(py)) => px >= py,
                _ => x >= y,
            };
            Some(if later { x } else { y })
        }
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// Parse the assorted timestamp shapes the alert tables store: RFC3339 (engine
/// writes `Utc::now().to_rfc3339()`), SQLite `YYYY-MM-DD HH:MM:SS` (table
/// default), or a bare `YYYY-MM-DD`. UTC throughout.
fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
    }
    None
}

/// Humanize the gap between a timestamp and `now` as e.g. "3d 4h ago".
fn humanize_since(ts: &str, now: DateTime<Utc>) -> String {
    let Some(then) = parse_ts(ts) else {
        return ts.to_string();
    };
    let secs = (now - then).num_seconds();
    if secs < 0 {
        return "in the future".to_string();
    }
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    let out = if days > 0 {
        format!("{days}d {}h", hours % 24)
    } else if hours > 0 {
        format!("{hours}h {}m", mins % 60)
    } else {
        format!("{mins}m")
    };
    format!("{out} ago")
}

fn print_json(rows: &[TrackedRow]) {
    let (bottom, top) = rows.iter().fold((0usize, 0usize), |(b, t), r| {
        if r.polarity == "top" {
            (b, t + 1)
        } else {
            (b + 1, t)
        }
    });
    let close = rows.iter().filter(|r| is_close(r)).count();
    let fired = rows.iter().filter(|r| r.fired).count();
    let payload = json!({
        "summary": {
            "total": rows.len(),
            "bottom": bottom,
            "top": top,
            "fired": fired,
            "close_to_firing": close,
        },
        "signals": rows,
    });
    let payload = cli_json::envelope(
        payload,
        "analytics cycles tracked",
        &Utc::now().to_rfc3339(),
        None,
    );
    match serde_json::to_string_pretty(&payload) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("failed to serialize tracked signals: {e}"),
    }
}

/// "Close to firing": an armed confluence row currently within one criterion of
/// its target (and not already met). Single criterion/component rows are not
/// counted as "close" — they are binary with no robust nearness measure here.
fn is_close(r: &TrackedRow) -> bool {
    r.shape == "confluence"
        && r.live.met == Some(false)
        && matches!(r.live.distance_to_target, Some(d) if d <= 1.0)
}

fn print_text(rows: &[TrackedRow], asset_filter: Option<&str>, polarity: Option<Polarity>) {
    let mut header = String::from("Tracked Cycle Signals");
    if let Some(a) = asset_filter {
        header.push_str(&format!(" — {}", a.to_uppercase()));
    }
    if let Some(p) = polarity {
        header.push_str(&format!(" [{} only]", polarity_label(p)));
    }
    println!("{header}");

    if rows.is_empty() {
        println!(
            "  no armed cycle-signal alerts match. Arm one with e.g.\n  \
             `pftui analytics alerts add --symbol BTC --condition cycle_bottom_monthly_4 ...`"
        );
        return;
    }

    let (bottom, top) = rows.iter().fold((0usize, 0usize), |(b, t), r| {
        if r.polarity == "top" {
            (b, t + 1)
        } else {
            (b + 1, t)
        }
    });
    let close = rows.iter().filter(|r| is_close(r)).count();
    let fired = rows.iter().filter(|r| r.fired).count();
    println!(
        "  {} tracked · {bottom} bottom / {top} top · {fired} fired · {close} close to firing",
        rows.len()
    );
    println!();

    // Group by asset for readability.
    let mut current_asset = String::new();
    for r in rows {
        if r.asset != current_asset {
            if !current_asset.is_empty() {
                println!();
            }
            current_asset = r.asset.clone();
            println!("  {current_asset}");
        }
        let glyph = if r.fired { "●" } else { "○" };
        println!(
            "  {glyph} [{}] {:<6} {:<10} {:<34} target {:<6}",
            r.alert_id, r.polarity, r.timeframe, truncate(&r.label, 34), r.target
        );
        println!(
            "        live: {:<38} | {}",
            truncate(&r.live.summary, 38),
            status_blurb(r)
        );
    }
    println!();
    println!("  ● fired   ○ armed (never fired)");
}

fn status_blurb(r: &TrackedRow) -> String {
    if r.fired {
        format!(
            "fired {}× · last {} · {}",
            r.fire_count.max(1),
            r.last_fired.as_deref().unwrap_or("?"),
            r.time_since_last
        )
    } else {
        format!("armed {} · never fired · {}", r.armed_at, recur_note(r))
    }
}

fn recur_note(r: &TrackedRow) -> String {
    if r.recurring {
        format!("recurring/{}m", r.cooldown_minutes)
    } else {
        "one-shot".to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::AlertStatus;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;

    fn backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: open_in_memory(),
        }
    }

    fn add_cycle_alert(
        backend: &BackendConnection,
        symbol: &str,
        condition: &str,
        recurring: bool,
    ) -> i64 {
        alerts::add_alert_backend(
            backend,
            alerts::NewAlert {
                kind: "technical",
                symbol,
                direction: "above",
                condition: Some(condition),
                threshold: "0",
                rule_text: &format!("{symbol} {condition}"),
                recurring,
                cooldown_minutes: 60,
            },
        )
        .unwrap()
    }

    /// Build the decoded rows the way `run` does, but without the JSON/text
    /// print, so assertions can inspect the structured rows directly.
    fn build_rows(
        backend: &BackendConnection,
        asset_filter: Option<&str>,
        polarity_filter: Option<Polarity>,
    ) -> Vec<TrackedRow> {
        let rules = alerts::list_alerts_backend(backend).unwrap();
        let triggered =
            triggered_alerts::list_triggered_alerts_backend(backend, None, false).unwrap();
        let mut fires: HashMap<i64, FireHistory> = HashMap::new();
        for t in &triggered {
            let entry = fires.entry(t.alert_id).or_insert(FireHistory {
                count: 0,
                last_fired: None,
            });
            entry.count += 1;
            match &entry.last_fired {
                Some(prev) if prev.as_str() >= t.triggered_at.as_str() => {}
                _ => entry.last_fired = Some(t.triggered_at.clone()),
            }
        }
        let now = Utc::now();
        let mut read_cache = HashMap::new();
        let mut rows = Vec::new();
        for rule in &rules {
            let Some(condition) = rule.condition.as_deref() else {
                continue;
            };
            if !csa::is_cycle_signal_condition(condition) {
                continue;
            }
            let Ok(parsed) = parse_condition(condition) else {
                continue;
            };
            let polarity = condition_polarity(condition).unwrap_or(Polarity::Bottom);
            if let Some(want) = polarity_filter {
                if want != polarity {
                    continue;
                }
            }
            if let Some(filter) = asset_filter {
                if !symbol_matches(filter, &rule.symbol) {
                    continue;
                }
            }
            let timeframe = condition_timeframe(&parsed);
            let live = compute_live_read(
                backend,
                &mut read_cache,
                &rule.symbol,
                timeframe,
                polarity,
                &parsed,
            );
            let fire = fires.get(&rule.id);
            let last_fired =
                newest(fire.and_then(|f| f.last_fired.clone()), rule.triggered_at.clone());
            let fired = fire.map(|f| f.count > 0).unwrap_or(false)
                || rule.triggered_at.is_some()
                || rule.status != AlertStatus::Armed;
            let fire_count = fire.map(|f| f.count).unwrap_or(0);
            let time_since_last = match &last_fired {
                Some(ts) => humanize_since(ts, now),
                None => "never".to_string(),
            };
            rows.push(TrackedRow {
                alert_id: rule.id,
                symbol: rule.symbol.clone(),
                asset: csa::friendly_asset(&rule.symbol),
                polarity: polarity_label(polarity).to_string(),
                shape: shape_label(&parsed).to_string(),
                label: condition_label(&parsed, polarity),
                timeframe: timeframe.label().to_string(),
                target: target_label(&parsed),
                status: rule.status.to_string(),
                armed_at: rule.created_at.clone(),
                recurring: rule.recurring,
                cooldown_minutes: rule.cooldown_minutes,
                fired,
                last_fired,
                time_since_last,
                fire_count,
                live,
            });
        }
        rows
    }

    #[test]
    fn fired_row_reports_label_timeframe_polarity_and_time_since() {
        let backend = backend();
        let bottom_id = add_cycle_alert(&backend, "BTC", "cycle_bottom_monthly_4", true);
        let top_id = add_cycle_alert(
            &backend,
            "BTC",
            "cycle_top_criterion_weekly_trend_line_lost",
            true,
        );

        // Simulate a firing event ~3 days 4 hours ago on the TOP criterion.
        let fired_at = (Utc::now() - chrono::Duration::seconds(3 * 86400 + 4 * 3600)).to_rfc3339();
        triggered_alerts::add_triggered_alert_backend(
            &backend,
            top_id,
            &fired_at,
            "{\"kind\":\"cycle_top_criterion\"}",
        )
        .unwrap();

        let rows = build_rows(&backend, None, None);
        assert_eq!(rows.len(), 2, "both cycle alerts decoded");

        let top = rows.iter().find(|r| r.alert_id == top_id).unwrap();
        assert_eq!(top.polarity, "top");
        assert_eq!(top.timeframe, "weekly");
        assert_eq!(top.shape, "criterion");
        assert_eq!(top.label, "trend line lost");
        assert!(top.fired, "top criterion should be fired");
        assert_eq!(top.fire_count, 1);
        assert!(
            top.time_since_last.starts_with("3d 4h"),
            "time-since should be ~3d 4h, got {}",
            top.time_since_last
        );

        let bottom = rows.iter().find(|r| r.alert_id == bottom_id).unwrap();
        assert_eq!(bottom.polarity, "bottom");
        assert_eq!(bottom.timeframe, "monthly");
        assert_eq!(bottom.shape, "confluence");
        assert_eq!(bottom.target, "≥4/7");
    }

    #[test]
    fn never_fired_row_reports_false_and_never() {
        let backend = backend();
        add_cycle_alert(&backend, "GC=F", "cycle_bottom_weekly_3", false);
        let rows = build_rows(&backend, None, None);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert!(!r.fired, "an untriggered rule must report fired=false");
        assert_eq!(r.fire_count, 0);
        assert_eq!(r.time_since_last, "never");
        assert!(r.last_fired.is_none());
        // No price history seeded → live read degrades gracefully, no panic.
        assert_eq!(r.live.met, None);
        assert_eq!(r.live.summary, "no price history");
    }

    #[test]
    fn non_cycle_alert_is_excluded() {
        let backend = backend();
        // A plain price alert (no cycle condition) must NOT appear.
        alerts::add_alert_backend(
            &backend,
            alerts::NewAlert {
                kind: "price",
                symbol: "BTC",
                direction: "above",
                condition: None,
                threshold: "100000",
                rule_text: "BTC above 100000",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        // A technical-but-not-cycle condition must also be excluded.
        alerts::add_alert_backend(
            &backend,
            alerts::NewAlert {
                kind: "technical",
                symbol: "BTC",
                direction: "below",
                condition: Some("price_below_sma200"),
                threshold: "0",
                rule_text: "BTC below sma200",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        add_cycle_alert(&backend, "BTC", "cycle_bottom_monthly_5", false);

        let rows = build_rows(&backend, None, None);
        assert_eq!(rows.len(), 1, "only the cycle alert is tracked");
        assert_eq!(rows[0].label, "confluence ≥5/7");
    }

    #[test]
    fn polarity_and_asset_filters_apply() {
        let backend = backend();
        add_cycle_alert(&backend, "BTC", "cycle_bottom_monthly_4", false);
        add_cycle_alert(&backend, "BTC", "cycle_top_monthly_4", false);
        add_cycle_alert(&backend, "GC=F", "cycle_bottom_weekly_3", false);

        let top_only = build_rows(&backend, None, Some(Polarity::Top));
        assert_eq!(top_only.len(), 1);
        assert_eq!(top_only[0].polarity, "top");

        // gold alias resolves to GC=F.
        let gold_only = build_rows(&backend, Some("gold"), None);
        assert_eq!(gold_only.len(), 1);
        assert_eq!(gold_only[0].symbol, "GC=F");

        // BTC and BTC-USD are treated as the same base asset.
        let btc = build_rows(&backend, Some("BTC-USD"), None);
        assert_eq!(btc.len(), 2);
    }

    #[test]
    fn json_shape_is_stable() {
        let backend = backend();
        let id = add_cycle_alert(&backend, "BTC", "cycle_bottom_monthly_4", true);
        triggered_alerts::add_triggered_alert_backend(
            &backend,
            id,
            &Utc::now().to_rfc3339(),
            "{}",
        )
        .unwrap();
        let rows = build_rows(&backend, None, None);
        let val = serde_json::to_value(&rows).unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let row = &arr[0];
        // Stable key contract for agent consumers.
        for key in [
            "alert_id",
            "symbol",
            "asset",
            "polarity",
            "shape",
            "label",
            "timeframe",
            "target",
            "status",
            "armed_at",
            "recurring",
            "cooldown_minutes",
            "fired",
            "time_since_last",
            "fire_count",
            "live",
        ] {
            assert!(row.get(key).is_some(), "missing JSON key {key}");
        }
        assert!(row["live"].get("summary").is_some());
    }

    #[test]
    fn humanize_since_formats() {
        let now = Utc::now();
        let three_d = (now - chrono::Duration::seconds(3 * 86400 + 4 * 3600)).to_rfc3339();
        assert!(humanize_since(&three_d, now).starts_with("3d 4h"));
        let mins = (now - chrono::Duration::minutes(5)).to_rfc3339();
        assert_eq!(humanize_since(&mins, now), "5m ago");
        let secs = (now - chrono::Duration::seconds(10)).to_rfc3339();
        assert_eq!(humanize_since(&secs, now), "just now");
        // SQLite naive format parses too.
        let naive = (now - chrono::Duration::hours(2))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert!(humanize_since(&naive, now).ends_with("ago"));
    }

    #[test]
    fn newest_compares_parsed_instants_not_raw_strings() {
        // RFC3339 'T'(0x54) > SQLite ' '(0x20) lexically, so a lexical compare
        // would wrongly pick the OLDER naive string when the RFC3339 one is in
        // an EARLIER calendar slot. Here the naive value is later in time and
        // must win despite sorting first lexically.
        // Same calendar day so the date/time separator decides the lexical
        // order: RFC3339 midnight ('T') sorts AFTER the naive noon (' ') even
        // though noon is the later instant.
        let rfc_earlier = "2026-06-10T00:00:00+00:00".to_string(); // midnight
        let naive_later = "2026-06-10 12:00:00".to_string(); // noon (later)
        assert!(rfc_earlier.as_str() > naive_later.as_str(), "precondition: lexical mis-ranks");
        assert_eq!(
            newest(Some(rfc_earlier.clone()), Some(naive_later.clone())),
            Some(naive_later),
            "newest must pick the chronologically-later naive timestamp"
        );
    }

    // ---- Component fallback-key live read (regression for the divergence) ----
    //
    // `pi_cycle_bottom` / `erf_positive` (bottom) and `pi_cycle_top` /
    // `erf_negative` (top) are NOT carried in `criteria[].components`, so a
    // hand-decode of the components map alone would report them "not met". The
    // dashboard must instead inherit the engine's met value (which applies the
    // `component_fallback`). These tests build a signal where the fallback key
    // IS met and assert the dashboard agrees with `evaluate` / `evaluate_top`.

    fn bottom_sig_fallback_met() -> CycleBottomSignals {
        CycleBottomSignals {
            symbol: "TEST".to_string(),
            timeframe: SignalTimeframe::Monthly,
            as_of: "2026-06-01".to_string(),
            rsi: None,
            rsi_ma: None,
            rsi_ma_turned_up: false,
            rsi_ma_cross_above_rsi: false,
            dss: None,
            dss_trigger: None,
            dss_turned_up: false,
            dss_cross_above_trigger: false,
            dss_oversold: false,
            erf: Some(1.5),
            erf_positive: true, // fallback-only key, met
            erf_green: true,
            erf_bottom_zone: false,
            erf_turned_up: false,
            cyberbands_state: None,
            cyberbands_bullish: false,
            cyberdots_weekly_strength: None,
            cyberdots_monthly_strength: None,
            cyberdots_bullish: false,
            cyberline_value: None,
            cyberline_price_above: None,
            cyberline_reclaim: false,
            pi_cycle_bottom: true, // fallback-only key (BonusSignal), met
            pi_cycle_last_bottom: Some("2026-05-01".to_string()),
            criteria: Vec::new(), // deliberately NO components carrying those keys
            core_watch: Vec::new(),
            met_count: 0,
            total: 7,
            bonus: None,
            verdict: String::new(),
        }
    }

    fn top_sig_fallback_met() -> CycleTopSignals {
        CycleTopSignals {
            symbol: "TEST".to_string(),
            timeframe: SignalTimeframe::Monthly,
            as_of: "2026-06-01".to_string(),
            rsi: None,
            rsi_ma: None,
            rsi_ma_turned_down: false,
            rsi_ma_cross_below_rsi: false,
            dss: None,
            dss_trigger: None,
            dss_turned_down: false,
            dss_cross_below_trigger: false,
            dss_overbought: false,
            erf: Some(-1.5),
            erf_negative: true, // fallback-only key, met
            erf_top_zone: false,
            erf_turned_down: false,
            cyberbands_state: None,
            cyberbands_bearish: false,
            cyberdots_weekly_down_strength: None,
            cyberdots_monthly_down_strength: None,
            cyberdots_bearish: false,
            cyberline_value: None,
            cyberline_price_above: None,
            cyberline_lost: false,
            pi_cycle_top: true, // fallback-only key (BonusSignal), met
            pi_cycle_last_top: Some("2026-05-01".to_string()),
            criteria: Vec::new(),
            core_watch: Vec::new(),
            met_count: 0,
            total: 7,
            bonus: None,
            verdict: String::new(),
        }
    }

    #[test]
    fn component_fallback_key_met_matches_engine() {
        // BOTTOM: pi_cycle_bottom and erf_positive are met only via fallback.
        let sig = bottom_sig_fallback_met();
        let cached = CachedRead::Bottom(Some(sig.clone()));
        for key in ["pi_cycle_bottom", "erf_positive"] {
            let cond = format!("cycle_component_monthly_{key}");
            let parsed = parse_condition(&cond).unwrap();
            let engine_met = csa::evaluate(&sig.symbol, &parsed, Some(&sig)).is_triggered;
            assert!(engine_met, "engine should report {key} met via fallback");
            let live = decode_live(&cached, &parsed, Polarity::Bottom);
            assert_eq!(
                live.met,
                Some(engine_met),
                "dashboard met for {key} must match engine ({engine_met})"
            );
        }

        // TOP: pi_cycle_top and erf_negative, the symmetric mirror.
        let tsig = top_sig_fallback_met();
        let tcached = CachedRead::Top(Some(tsig.clone()));
        for key in ["pi_cycle_top", "erf_negative"] {
            let cond = format!("cycle_top_component_monthly_{key}");
            let parsed = parse_condition(&cond).unwrap();
            let engine_met = csa::evaluate_top(&tsig.symbol, &parsed, Some(&tsig)).is_triggered;
            assert!(engine_met, "engine should report {key} met via fallback");
            let live = decode_live(&tcached, &parsed, Polarity::Top);
            assert_eq!(
                live.met,
                Some(engine_met),
                "dashboard met for {key} must match engine ({engine_met})"
            );
        }
    }
}
