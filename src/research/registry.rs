//! Signal registry — canonical deterministic event emitters.
//!
//! A signal is `(canonical_id, version, description, emitter)`. The emitter
//! walks a daily price series and yields dated EVENTS — state *transitions*,
//! never states ("weekly structure flipped to DOWNTREND on 2026-06-07", not
//! "is in a downtrend"). Every emitter draws on an existing deterministic
//! engine (`analytics::market_structure`, `analytics::cyber`,
//! `analytics::cycle_engine`, `indicators`); nothing here re-derives
//! indicator math.
//!
//! # Versioning rule
//!
//! Any change to an emitter's logic bumps its version string; persisted
//! expectancy stats bind to `(signal_id, signal_version)` so stale stats can
//! never be cited against a changed definition.
//!
//! # Walk-forward semantics
//!
//! Event DATES are lookahead-free: an event is dated at the bar where the
//! transition became OBSERVABLE in real time (a swing pivot's confirmation
//! bar, the close that broke a level, the bar a cycle age crossed into its
//! timing band) — never at the underlying extreme itself. Two documented
//! parameter-snapshot exceptions, both slowly-varying structural parameters
//! rather than tradable state: the cycle timing-band percentiles and the
//! FLD offset come from the cycle engine's full-sample (as-of-truncated)
//! statistics. Callers that need historical `as_of` correctness truncate
//! the history BEFORE building the [`AssetContext`] — then every input the
//! emitters see existed at `as_of`.
//!
//! # Performance
//!
//! [`AssetContext::build`] computes every shared series/engine pass exactly
//! once per asset (structure walks, one Cyber engine run, one cycle-engine
//! run, SMA200/RSI14); the ~27 emitters then only slice precomputed event
//! streams, so a full default backtest stays in seconds.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::analytics::cyber::{self, line::compute_line_crosses, CyberTimeframe};
use crate::analytics::cycle_engine::{self, CycleLow, CycleReport, DegreeStatus};
use crate::analytics::market_structure::{
    aggregate, classify_structure, Bar, StructureClass, Swing, SwingKind, Timeframe,
};
use crate::indicators::{compute_rsi, compute_sma};
use crate::models::price::HistoryRecord;

// ---------------------------------------------------------------------------
// Event + registry types
// ---------------------------------------------------------------------------

/// One dated signal event (a state transition observed at `date`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SignalFiring {
    /// Daily bar date (YYYY-MM-DD) at which the transition was observable.
    pub date: String,
    /// Human-readable context for the event.
    pub detail: String,
}

/// The emitter contract: walk the (precomputed) asset context and yield the
/// signal's dated events, oldest first.
pub trait SignalEmitter {
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn emit(&self, ctx: &AssetContext) -> Vec<SignalFiring>;
}

/// Registry row: a concrete emitter backed by an extraction fn over the
/// shared [`AssetContext`].
pub struct SignalDef {
    pub id: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    extract: fn(&AssetContext) -> Vec<SignalFiring>,
}

impl SignalEmitter for SignalDef {
    fn id(&self) -> &'static str {
        self.id
    }
    fn version(&self) -> &'static str {
        self.version
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn emit(&self, ctx: &AssetContext) -> Vec<SignalFiring> {
        (self.extract)(ctx)
    }
}

/// The registration table — every canonical signal, in stable order.
pub fn registry() -> &'static [SignalDef] {
    &REGISTRY
}

/// Look up one signal by canonical id.
pub fn find_signal(id: &str) -> Option<&'static SignalDef> {
    REGISTRY.iter().find(|d| d.id == id)
}

// ---------------------------------------------------------------------------
// Shared per-asset context (compute once, emit all signal streams)
// ---------------------------------------------------------------------------

/// Precomputed series + engine passes for one asset. Build once per asset;
/// every emitter slices from here.
pub struct AssetContext {
    /// Series actually analyzed (e.g. BTC → deep BTC-USD).
    pub series: String,
    pub dates: Vec<String>,
    pub closes: Vec<f64>,
    /// SMA(200) of daily closes (None during warm-up).
    pub sma200: Vec<Option<f64>>,
    rsi14: Vec<Option<f64>>,
    structure_daily: StructureStream,
    structure_weekly: StructureStream,
    cyber: Option<cyber::CyberSnapshot>,
    line_crosses: Vec<cyber::line::LineCross>,
    cycle: Option<CycleEvents>,
}

#[derive(Default)]
struct StructureStream {
    flips_up: Vec<SignalFiring>,
    flips_down: Vec<SignalFiring>,
    bos_support: Vec<SignalFiring>,
    bos_resistance: Vec<SignalFiring>,
}

#[derive(Default)]
struct CycleEvents {
    band_enter_daily: Vec<SignalFiring>,
    band_enter_intermediate: Vec<SignalFiring>,
    fld_cross_up: Vec<SignalFiring>,
    fld_cross_down: Vec<SignalFiring>,
    failed: Vec<SignalFiring>,
    vtl_break: Vec<SignalFiring>,
}

impl AssetContext {
    /// Build the shared context from DAILY history (oldest first, as the
    /// `price_history` getters return it). Returns None on empty history.
    /// For historical `as_of` studies, truncate `history` to `as_of` first.
    pub fn build(symbol: &str, series: &str, history: &[HistoryRecord]) -> Option<AssetContext> {
        if history.is_empty() {
            return None;
        }
        let dates: Vec<String> = history.iter().map(|r| r.date.clone()).collect();
        let closes: Vec<f64> = history
            .iter()
            .map(|r| r.close.to_f64().unwrap_or(0.0))
            .collect();
        let sma200 = compute_sma(&closes, 200);
        let rsi14 = compute_rsi(&closes, 14);

        let daily_bars = aggregate(history, Timeframe::Daily);
        let weekly_bars = aggregate(history, Timeframe::Weekly);
        let structure_daily =
            walk_structure(&daily_bars, Timeframe::Daily.pivot_window(), "daily");
        let structure_weekly =
            walk_structure(&weekly_bars, Timeframe::Weekly.pivot_window(), "weekly");

        // One full Cyber engine pass (daily bars; Pi Cycle internally daily).
        // The huge cap keeps every dated component stream uncapped.
        let cyber = cyber::analyze(series, CyberTimeframe::Daily, history, usize::MAX / 2);
        let line_crosses = compute_line_crosses(&closes, &dates);

        // One cycle-engine pass; derive dated cycle events from it.
        let cycle_cfg = cycle_engine::default_config(symbol, series);
        let cycle = cycle_engine::analyze(&cycle_cfg, history)
            .map(|report| derive_cycle_events(&report, history));

        Some(AssetContext {
            series: series.to_uppercase(),
            dates,
            closes,
            sma200,
            rsi14,
            structure_daily,
            structure_weekly,
            cyber,
            line_crosses,
            cycle,
        })
    }
}

// ---------------------------------------------------------------------------
// Market-structure stream (streaming, real-time pivot confirmation)
// ---------------------------------------------------------------------------

/// Streaming walk of the market-structure engine's semantics: a pivot at bar
/// `i` (close >= the N left closes, strictly > the N right closes; mirror
/// for lows) confirms at bar `i + n`; consecutive same-kind pivots compress
/// to the more extreme one (`market_structure::detect_swings` rules applied
/// incrementally). After each accepted swing, the running swing list is
/// re-classified with `market_structure::classify_structure`; transitions
/// INTO Uptrend/Downtrend emit flip events dated at the confirmation bar.
/// Break-of-structure: the active (most recent surviving) swing low/high is
/// broken by a close beyond it — dated at the breaking close, level
/// deactivates (matching `detect_breaks`).
fn walk_structure(bars: &[Bar], n: usize, tf_label: &str) -> StructureStream {
    let mut s = StructureStream::default();
    if n == 0 || bars.len() < 2 * n + 1 {
        return s;
    }
    let mut alternating: Vec<Swing> = Vec::new();
    let mut prev_class = StructureClass::Insufficient;
    // Active break levels: (level, swing date).
    let mut active_low: Option<(Decimal, String)> = None;
    let mut active_high: Option<(Decimal, String)> = None;

    for t in 2 * n..bars.len() {
        let i = t - n;
        let c = bars[i].close;
        let left = &bars[i - n..i];
        let right = &bars[i + 1..=i + n];
        let is_high = left.iter().all(|b| c >= b.close) && right.iter().all(|b| c > b.close);
        let is_low =
            !is_high && left.iter().all(|b| c <= b.close) && right.iter().all(|b| c < b.close);

        if is_high || is_low {
            let swing = Swing {
                date: bars[i].date.clone(),
                price: c,
                kind: if is_high { SwingKind::High } else { SwingKind::Low },
                label: None,
                bar_index: i,
            };
            let accepted = match alternating.last_mut() {
                Some(prev) if prev.kind == swing.kind => {
                    let replace = match swing.kind {
                        SwingKind::High => swing.price >= prev.price,
                        SwingKind::Low => swing.price <= prev.price,
                    };
                    if replace {
                        *prev = swing.clone();
                    }
                    replace
                }
                _ => {
                    alternating.push(swing.clone());
                    true
                }
            };
            if accepted {
                match swing.kind {
                    SwingKind::Low => active_low = Some((swing.price, swing.date.clone())),
                    SwingKind::High => active_high = Some((swing.price, swing.date.clone())),
                }
                let class = classify_structure(&alternating);
                if class != prev_class {
                    let firing = |label: &str| SignalFiring {
                        date: bars[t].date.clone(),
                        detail: format!(
                            "{tf_label} structure flipped to {label} (pivot {} {} confirmed)",
                            swing.date,
                            match swing.kind {
                                SwingKind::High => "high",
                                SwingKind::Low => "low",
                            }
                        ),
                    };
                    match class {
                        StructureClass::Uptrend => s.flips_up.push(firing("UPTREND")),
                        StructureClass::Downtrend => s.flips_down.push(firing("DOWNTREND")),
                        _ => {}
                    }
                    prev_class = class;
                }
            }
        }

        // Break-of-structure on the current bar's close.
        if let Some((level, swing_date)) = active_low.clone() {
            if bars[t].close < level {
                s.bos_support.push(SignalFiring {
                    date: bars[t].date.clone(),
                    detail: format!(
                        "{tf_label} close broke swing-low support {level} set {swing_date}"
                    ),
                });
                active_low = None;
            }
        }
        if let Some((level, swing_date)) = active_high.clone() {
            if bars[t].close > level {
                s.bos_resistance.push(SignalFiring {
                    date: bars[t].date.clone(),
                    detail: format!(
                        "{tf_label} close broke swing-high resistance {level} set {swing_date}"
                    ),
                });
                active_high = None;
            }
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Cycle events (derived from one cycle_engine::analyze pass)
// ---------------------------------------------------------------------------

fn confirm_index(low: &CycleLow, dates: &[String]) -> usize {
    low.confirmed_date
        .as_deref()
        .and_then(|d| dates.iter().position(|x| x == d))
        .unwrap_or(low.index + 1)
}

fn degree_named<'a>(report: &'a CycleReport, names: &[&str]) -> Option<&'a DegreeStatus> {
    report
        .degrees
        .iter()
        .find(|d| names.iter().any(|n| d.degree == *n))
}

/// Shortest degree = most events; the engine orders degrees longest first.
fn shortest_degree(report: &CycleReport) -> Option<&DegreeStatus> {
    report.degrees.last()
}

fn derive_cycle_events(report: &CycleReport, history: &[HistoryRecord]) -> CycleEvents {
    let dates: Vec<&str> = history.iter().map(|r| r.date.as_str()).collect();
    let dates_owned: Vec<String> = history.iter().map(|r| r.date.clone()).collect();
    let closes: Vec<Decimal> = history.iter().map(|r| r.close).collect();
    let lows: Vec<Decimal> = history
        .iter()
        .map(|r| r.low.unwrap_or(r.close))
        .collect();
    let highs: Vec<Decimal> = history
        .iter()
        .map(|r| r.high.unwrap_or(r.close))
        .collect();
    let two = Decimal::from(2);
    let hl2: Vec<Decimal> = highs
        .iter()
        .zip(lows.iter())
        .map(|(h, l)| (*h + *l) / two)
        .collect();

    let band_enter_daily = degree_named(report, &["daily"])
        .map(|d| band_enter_events(d, &dates))
        .unwrap_or_default();
    let band_enter_intermediate = degree_named(report, &["intermediate", "investor"])
        .map(|d| band_enter_events(d, &dates))
        .unwrap_or_default();

    let (fld_cross_up, fld_cross_down, failed, vtl_break) = match shortest_degree(report) {
        Some(deg) => {
            let offset = deg.expected_len_bars / 2;
            let (up, down) = fld_cross_events(deg, offset, &dates_owned, &closes, &hl2);
            (
                up,
                down,
                failed_cycle_events(deg, &dates_owned, &closes),
                vtl_break_events(deg, &dates_owned, &closes, &lows),
            )
        }
        None => Default::default(),
    };

    CycleEvents {
        band_enter_daily,
        band_enter_intermediate,
        fld_cross_up,
        fld_cross_down,
        failed,
        vtl_break,
    }
}

/// Price (cycle age) enters the degree's timing band: dated at the bar where
/// `age == ceil(band_lo_bars)` from each confirmed low. The band percentiles
/// are the engine's trailing-window stats over the supplied (as-of-truncated)
/// history — a documented parameter-snapshot, not per-bar recomputation.
fn band_enter_events(deg: &DegreeStatus, dates: &[&str]) -> Vec<SignalFiring> {
    let Some(band) = &deg.band else {
        return Vec::new();
    };
    let lo = band.band_lo_bars.ceil() as usize;
    if lo == 0 {
        return Vec::new();
    }
    let confirmed: Vec<&CycleLow> = deg.all_lows.iter().filter(|l| l.confirmed).collect();
    let mut out = Vec::new();
    for (k, low) in confirmed.iter().enumerate() {
        let entry = low.index + lo;
        let next_idx = confirmed
            .get(k + 1)
            .map(|l| l.index)
            .unwrap_or(usize::MAX);
        if entry >= dates.len() || entry >= next_idx {
            continue;
        }
        // The low itself must have been confirmed by the entry bar.
        let cidx = low
            .confirmed_date
            .as_deref()
            .and_then(|d| dates.iter().position(|x| *x == d))
            .unwrap_or(low.index + 1);
        if cidx > entry {
            continue;
        }
        out.push(SignalFiring {
            date: dates[entry].to_string(),
            detail: format!(
                "{} cycle age entered timing band ({:.0}-{:.0} bars) from low {}",
                deg.degree, band.band_lo_bars, band.band_hi_bars, low.date
            ),
        });
    }
    out
}

/// All FLD crosses (close vs hl2 displaced forward by `offset` bars), same
/// side semantics as `cycle_engine::compute_fld`: close > FLD → above,
/// close < FLD → below, equality keeps the previous side. The offset is the
/// engine's expected-length snapshot (documented above).
fn fld_cross_events(
    deg: &DegreeStatus,
    offset: usize,
    dates: &[String],
    closes: &[Decimal],
    hl2: &[Decimal],
) -> (Vec<SignalFiring>, Vec<SignalFiring>) {
    let n = closes.len();
    if offset == 0 || n < offset + 2 {
        return (Vec::new(), Vec::new());
    }
    let mut up = Vec::new();
    let mut down = Vec::new();
    let mut side: Option<bool> = None;
    for i in offset..n {
        let f = hl2[i - offset];
        let c = closes[i];
        let new_side = if c > f {
            Some(true)
        } else if c < f {
            Some(false)
        } else {
            side
        };
        if let (Some(prev), Some(cur)) = (side, new_side) {
            if prev != cur {
                let firing = SignalFiring {
                    date: dates[i].clone(),
                    detail: format!(
                        "close crossed {} the {} FLD (offset {} bars)",
                        if cur { "above" } else { "below" },
                        deg.degree,
                        offset
                    ),
                };
                if cur {
                    up.push(firing);
                } else {
                    down.push(firing);
                }
            }
        }
        side = new_side;
    }
    (up, down)
}

/// Failed-cycle ONSET (§13): first close below a confirmed cycle origin low,
/// after that low's confirmation bar and before the next confirmed low.
fn failed_cycle_events(
    deg: &DegreeStatus,
    dates: &[String],
    closes: &[Decimal],
) -> Vec<SignalFiring> {
    let confirmed: Vec<&CycleLow> = deg.all_lows.iter().filter(|l| l.confirmed).collect();
    let mut out = Vec::new();
    for (k, low) in confirmed.iter().enumerate() {
        let start = confirm_index(low, dates);
        let end = confirmed
            .get(k + 1)
            .map(|l| l.index)
            .unwrap_or(closes.len());
        for t in start..end.min(closes.len()) {
            if closes[t] < low.price {
                out.push(SignalFiring {
                    date: dates[t].clone(),
                    detail: format!(
                        "{} cycle FAILED: close below origin low {} ({})",
                        deg.degree, low.date, low.price
                    ),
                });
                break;
            }
        }
    }
    out
}

/// VTL break ONSET (§12): for each consecutive pair of confirmed lows, a
/// valid VTL (the line never cuts bar lows between its anchors) is broken by
/// the first close below it — scanned from the second anchor's confirmation
/// until the next confirmed low re-anchors the line.
fn vtl_break_events(
    deg: &DegreeStatus,
    dates: &[String],
    closes: &[Decimal],
    bar_lows: &[Decimal],
) -> Vec<SignalFiring> {
    let confirmed: Vec<&CycleLow> = deg.all_lows.iter().filter(|l| l.confirmed).collect();
    let mut out = Vec::new();
    for k in 0..confirmed.len().saturating_sub(1) {
        let a = confirmed[k];
        let b = confirmed[k + 1];
        if b.index <= a.index {
            continue;
        }
        let span = Decimal::from((b.index - a.index) as i64);
        let slope = (b.price - a.price) / span;
        let line_at =
            |i: usize| a.price + slope * Decimal::from(i as i64 - a.index as i64);
        let valid = (a.index + 1..b.index).all(|i| bar_lows[i] >= line_at(i));
        if !valid {
            continue;
        }
        let start = confirm_index(b, dates).max(b.index + 1);
        let end = confirmed
            .get(k + 2)
            .map(|l| l.index)
            .unwrap_or(closes.len());
        for t in start..end.min(closes.len()) {
            if closes[t] < line_at(t) {
                out.push(SignalFiring {
                    date: dates[t].clone(),
                    detail: format!(
                        "{} VTL (anchors {} / {}) broken by close",
                        deg.degree, a.date, b.date
                    ),
                });
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Threshold-state onsets (SMA200 / RSI / Mayer)
// ---------------------------------------------------------------------------

/// Onset events of a boolean state series: emits where the state turns true
/// after being false/undefined (transition, not state). Undefined indicator
/// bars count as "not in state".
fn onset_events(
    dates: &[String],
    state: impl Fn(usize) -> Option<bool>,
    detail: impl Fn(usize) -> String,
) -> Vec<SignalFiring> {
    let mut out = Vec::new();
    let mut prev = false;
    for (i, date) in dates.iter().enumerate() {
        let cur = state(i).unwrap_or(false);
        if cur && !prev {
            out.push(SignalFiring {
                date: date.clone(),
                detail: detail(i),
            });
        }
        prev = cur;
    }
    out
}

// ---------------------------------------------------------------------------
// Per-signal extraction fns
// ---------------------------------------------------------------------------

fn x_structure_daily_flip_up(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.structure_daily.flips_up.clone()
}
fn x_structure_daily_flip_down(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.structure_daily.flips_down.clone()
}
fn x_structure_weekly_flip_up(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.structure_weekly.flips_up.clone()
}
fn x_structure_weekly_flip_down(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.structure_weekly.flips_down.clone()
}
fn x_structure_daily_bos_support(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.structure_daily.bos_support.clone()
}
fn x_structure_daily_bos_resistance(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.structure_daily.bos_resistance.clone()
}

fn qb_flips(ctx: &AssetContext, to: cyber::bands::QbState) -> Vec<SignalFiring> {
    ctx.cyber
        .as_ref()
        .and_then(|c| c.bands_gaussian.as_ref())
        .map(|b| {
            b.transitions
                .iter()
                .filter(|t| t.to == to)
                .map(|t| SignalFiring {
                    date: t.date.clone(),
                    detail: format!("QB state {} → {}", t.from.label(), t.to.label()),
                })
                .collect()
        })
        .unwrap_or_default()
}
fn x_cyber_qb_flip_bull(ctx: &AssetContext) -> Vec<SignalFiring> {
    qb_flips(ctx, cyber::bands::QbState::Bullish)
}
fn x_cyber_qb_flip_bear(ctx: &AssetContext) -> Vec<SignalFiring> {
    qb_flips(ctx, cyber::bands::QbState::Bearish)
}

fn x_cyber_dot_up_strength3(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cyber
        .as_ref()
        .and_then(|c| c.dots.as_ref())
        .map(|d| {
            d.recent_dots
                .iter()
                .filter(|e| e.direction == "up" && e.strength >= 3)
                .map(|e| SignalFiring {
                    date: e.date.clone(),
                    detail: "up dot run onset at strength 3/3".to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn line_crosses(ctx: &AssetContext, direction: &str) -> Vec<SignalFiring> {
    ctx.line_crosses
        .iter()
        .filter(|c| c.direction == direction)
        .map(|c| SignalFiring {
            date: c.date.clone(),
            detail: format!("price crossed {} the CyberLine", c.direction),
        })
        .collect()
}
fn x_cyberline_cross_up(ctx: &AssetContext) -> Vec<SignalFiring> {
    line_crosses(ctx, "above")
}
fn x_cyberline_cross_down(ctx: &AssetContext) -> Vec<SignalFiring> {
    line_crosses(ctx, "below")
}

fn x_cyber_pi_top_fire(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cyber
        .as_ref()
        .and_then(|c| c.pi_cycle.as_ref())
        .map(|p| {
            p.top_fires
                .iter()
                .map(|d| SignalFiring {
                    date: d.clone(),
                    detail: "Pi Cycle TOP fired (SMA111 rose through 2x SMA350)".to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}
fn x_cyber_pi_bottom_fire(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cyber
        .as_ref()
        .and_then(|c| c.pi_cycle.as_ref())
        .map(|p| {
            p.bottom_fires
                .iter()
                .map(|d| SignalFiring {
                    date: d.clone(),
                    detail: "Pi Cycle BOTTOM fired (0.745x SMA471 crossed above EMA150)"
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mtf_zone_enter(ctx: &AssetContext, green: bool) -> Vec<SignalFiring> {
    let Some(m) = ctx.cyber.as_ref().and_then(|c| c.mtf_rsi.as_ref()) else {
        return Vec::new();
    };
    let series = if green { &m.green_series } else { &m.red_series };
    // Cyber ran on daily bars, so the bar index maps 1:1 onto ctx.dates.
    let mut out = Vec::new();
    let mut prev = false;
    for (i, &cur) in series.iter().enumerate() {
        if cur && !prev && i < ctx.dates.len() {
            out.push(SignalFiring {
                date: ctx.dates[i].clone(),
                detail: format!(
                    "MTF RSI entered the {} zone (all gating timeframes {})",
                    if green { "green" } else { "red" },
                    if green { "oversold" } else { "overbought" }
                ),
            });
        }
        prev = cur;
    }
    out
}
fn x_cyber_mtf_rsi_green_enter(ctx: &AssetContext) -> Vec<SignalFiring> {
    mtf_zone_enter(ctx, true)
}
fn x_cyber_mtf_rsi_red_enter(ctx: &AssetContext) -> Vec<SignalFiring> {
    mtf_zone_enter(ctx, false)
}

fn breakouts(ctx: &AssetContext, direction: &str) -> Vec<SignalFiring> {
    ctx.cyber
        .as_ref()
        .and_then(|c| c.breakout.as_ref())
        .map(|b| {
            b.recent
                .iter()
                .filter(|e| e.direction == direction)
                .map(|e| SignalFiring {
                    date: e.date.clone(),
                    detail: format!(
                        "{} breakout arrow ({})",
                        e.direction,
                        e.signals.join("+")
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}
fn x_cyber_breakout_bull(ctx: &AssetContext) -> Vec<SignalFiring> {
    breakouts(ctx, "bull")
}
fn x_cyber_breakout_bear(ctx: &AssetContext) -> Vec<SignalFiring> {
    breakouts(ctx, "bear")
}

fn x_cycle_band_enter_daily(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cycle
        .as_ref()
        .map(|c| c.band_enter_daily.clone())
        .unwrap_or_default()
}
fn x_cycle_band_enter_intermediate(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cycle
        .as_ref()
        .map(|c| c.band_enter_intermediate.clone())
        .unwrap_or_default()
}
fn x_cycle_fld_cross_up(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cycle
        .as_ref()
        .map(|c| c.fld_cross_up.clone())
        .unwrap_or_default()
}
fn x_cycle_fld_cross_down(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cycle
        .as_ref()
        .map(|c| c.fld_cross_down.clone())
        .unwrap_or_default()
}
fn x_cycle_failed(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cycle.as_ref().map(|c| c.failed.clone()).unwrap_or_default()
}
fn x_cycle_vtl_break(ctx: &AssetContext) -> Vec<SignalFiring> {
    ctx.cycle
        .as_ref()
        .map(|c| c.vtl_break.clone())
        .unwrap_or_default()
}

fn x_extension_20pct_over_200dma(ctx: &AssetContext) -> Vec<SignalFiring> {
    onset_events(
        &ctx.dates,
        |i| ctx.sma200[i].map(|s| s > 0.0 && ctx.closes[i] > s * 1.20),
        |i| {
            format!(
                "close extended >20% above the 200dma ({:.1}%)",
                ctx.sma200[i]
                    .map(|s| (ctx.closes[i] / s - 1.0) * 100.0)
                    .unwrap_or(0.0)
            )
        },
    )
}

fn x_window_within_5pct_200dma(ctx: &AssetContext) -> Vec<SignalFiring> {
    onset_events(
        &ctx.dates,
        |i| {
            ctx.sma200[i].map(|s| s > 0.0 && (ctx.closes[i] / s - 1.0).abs() <= 0.05)
        },
        |i| {
            format!(
                "close entered the +/-5% window around the 200dma ({:+.1}%)",
                ctx.sma200[i]
                    .map(|s| (ctx.closes[i] / s - 1.0) * 100.0)
                    .unwrap_or(0.0)
            )
        },
    )
}

fn x_rsi14_oversold_25_enter(ctx: &AssetContext) -> Vec<SignalFiring> {
    onset_events(
        &ctx.dates,
        |i| ctx.rsi14[i].map(|r| r < 25.0),
        |i| {
            format!(
                "RSI(14) entered oversold <25 ({:.1})",
                ctx.rsi14[i].unwrap_or(0.0)
            )
        },
    )
}

fn x_mayer_under_085_enter(ctx: &AssetContext) -> Vec<SignalFiring> {
    onset_events(
        &ctx.dates,
        |i| ctx.sma200[i].map(|s| s > 0.0 && ctx.closes[i] / s < 0.85),
        |i| {
            format!(
                "Mayer multiple entered <0.85 ({:.3})",
                ctx.sma200[i].map(|s| ctx.closes[i] / s).unwrap_or(0.0)
            )
        },
    )
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

static REGISTRY: [SignalDef; 27] = [
    SignalDef {
        id: "structure_daily_flip_up",
        version: "1",
        description: "Daily market structure classification flips to UPTREND (swing-confirmed)",
        extract: x_structure_daily_flip_up,
    },
    SignalDef {
        id: "structure_daily_flip_down",
        version: "1",
        description: "Daily market structure classification flips to DOWNTREND (swing-confirmed)",
        extract: x_structure_daily_flip_down,
    },
    SignalDef {
        id: "structure_weekly_flip_up",
        version: "1",
        description: "Weekly market structure classification flips to UPTREND (swing-confirmed)",
        extract: x_structure_weekly_flip_up,
    },
    SignalDef {
        id: "structure_weekly_flip_down",
        version: "1",
        description: "Weekly market structure classification flips to DOWNTREND (swing-confirmed)",
        extract: x_structure_weekly_flip_down,
    },
    SignalDef {
        id: "structure_daily_bos_support",
        version: "1",
        description: "Daily close breaks the active swing-low support (break of structure)",
        extract: x_structure_daily_bos_support,
    },
    SignalDef {
        id: "structure_daily_bos_resistance",
        version: "1",
        description: "Daily close breaks the active swing-high resistance (break of structure)",
        extract: x_structure_daily_bos_resistance,
    },
    SignalDef {
        id: "cyber_qb_flip_bull",
        version: "1",
        description: "Cyber Gaussian-channel QB state machine flips bullish (daily bars)",
        extract: x_cyber_qb_flip_bull,
    },
    SignalDef {
        id: "cyber_qb_flip_bear",
        version: "1",
        description: "Cyber Gaussian-channel QB state machine flips bearish (daily bars)",
        extract: x_cyber_qb_flip_bear,
    },
    SignalDef {
        id: "cyber_dot_up_strength3",
        version: "1",
        description: "Cyber up-dot run onset at full strength 3/3 (daily bars)",
        extract: x_cyber_dot_up_strength3,
    },
    SignalDef {
        id: "cyberline_cross_up",
        version: "1",
        description: "Price crosses above the CyberLine (VIDYA len 18, daily bars)",
        extract: x_cyberline_cross_up,
    },
    SignalDef {
        id: "cyberline_cross_down",
        version: "1",
        description: "Price crosses below the CyberLine (VIDYA len 18, daily bars)",
        extract: x_cyberline_cross_down,
    },
    SignalDef {
        id: "cyber_pi_top_fire",
        version: "1",
        description: "Pi Cycle TOP fires: SMA111 rises through 2x SMA350 (daily closes)",
        extract: x_cyber_pi_top_fire,
    },
    SignalDef {
        id: "cyber_pi_bottom_fire",
        version: "1",
        description: "Pi Cycle BOTTOM fires: 0.745x SMA471 crosses above EMA150 (daily closes)",
        extract: x_cyber_pi_bottom_fire,
    },
    SignalDef {
        id: "cyber_mtf_rsi_green_enter",
        version: "1",
        description: "MTF RSI(6) enters the green zone — all gating timeframes oversold (daily run)",
        extract: x_cyber_mtf_rsi_green_enter,
    },
    SignalDef {
        id: "cyber_mtf_rsi_red_enter",
        version: "1",
        description: "MTF RSI(6) enters the red zone — all gating timeframes overbought (daily run)",
        extract: x_cyber_mtf_rsi_red_enter,
    },
    SignalDef {
        id: "cyber_breakout_bull",
        version: "1",
        description: "Cyber hybrid bull breakout arrow (RSI zone exit / 3-line strike / exhaustion)",
        extract: x_cyber_breakout_bull,
    },
    SignalDef {
        id: "cyber_breakout_bear",
        version: "1",
        description: "Cyber hybrid bear breakout arrow (RSI zone exit / 3-line strike / exhaustion)",
        extract: x_cyber_breakout_bear,
    },
    SignalDef {
        id: "cycle_band_enter_daily",
        version: "1",
        description: "Cycle age enters the DAILY degree's P15-P85 timing band (cycle engine)",
        extract: x_cycle_band_enter_daily,
    },
    SignalDef {
        id: "cycle_band_enter_intermediate",
        version: "1",
        description:
            "Cycle age enters the INTERMEDIATE/INVESTOR degree's P15-P85 timing band (cycle engine)",
        extract: x_cycle_band_enter_intermediate,
    },
    SignalDef {
        id: "cycle_fld_cross_up",
        version: "1",
        description: "Close crosses above the shortest-degree FLD (trough confirmation, cycle engine)",
        extract: x_cycle_fld_cross_up,
    },
    SignalDef {
        id: "cycle_fld_cross_down",
        version: "1",
        description: "Close crosses below the shortest-degree FLD (peak confirmation, cycle engine)",
        extract: x_cycle_fld_cross_down,
    },
    SignalDef {
        id: "cycle_failed",
        version: "1",
        description: "Failed-cycle onset: first close below a confirmed cycle origin low (shortest degree)",
        extract: x_cycle_failed,
    },
    SignalDef {
        id: "cycle_vtl_break",
        version: "1",
        description: "Valid trend line (two confirmed lows) broken by a close (shortest degree)",
        extract: x_cycle_vtl_break,
    },
    SignalDef {
        id: "extension_20pct_over_200dma",
        version: "1",
        description: "Onset: close extends >20% above the 200-day SMA (standing rule 13 gate)",
        extract: x_extension_20pct_over_200dma,
    },
    SignalDef {
        id: "window_within_5pct_200dma",
        version: "1",
        description: "Onset: close enters the +/-5% window around the 200-day SMA",
        extract: x_window_within_5pct_200dma,
    },
    SignalDef {
        id: "rsi14_oversold_25_enter",
        version: "1",
        description: "Onset: RSI(14) crosses under 25 (deep oversold)",
        extract: x_rsi14_oversold_25_enter,
    },
    SignalDef {
        id: "mayer_under_085_enter",
        version: "1",
        description: "Onset: Mayer multiple (close / 200dma) drops under 0.85",
        extract: x_mayer_under_085_enter,
    },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Days, NaiveDate};
    use std::str::FromStr;

    fn record(date: &str, close: f64) -> HistoryRecord {
        HistoryRecord {
            date: date.to_string(),
            close: Decimal::from_str(&format!("{close:.4}")).unwrap_or_default(),
            volume: None,
            open: Decimal::from_str(&format!("{:.4}", close - 0.5)).ok(),
            high: Decimal::from_str(&format!("{:.4}", close + 1.0)).ok(),
            low: Decimal::from_str(&format!("{:.4}", close - 1.0)).ok(),
        }
    }

    fn series_from(closes: &[f64]) -> Vec<HistoryRecord> {
        let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap_or_default();
        closes
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let date = (start + Days::new(i as u64)).format("%Y-%m-%d").to_string();
                record(&date, *c)
            })
            .collect()
    }

    /// Up-leg, down-leg, up-leg sawtooth so swings + flips exist.
    fn wave(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + 20.0 * ((i as f64) / 25.0).sin() + i as f64 * 0.02)
            .collect()
    }

    #[test]
    fn registry_ids_unique_and_versioned() {
        let mut ids: Vec<&str> = registry().iter().map(|d| d.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate signal ids in registry");
        for d in registry() {
            assert!(!d.version.is_empty());
            assert!(!d.description.is_empty());
            assert!(d.id.chars().all(|c| c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '_'));
        }
    }

    #[test]
    fn structure_flips_are_transitions_not_states() {
        // Trending zigzags (the wave is strong enough to print pivots every
        // ~20 bars while the drift makes highs AND lows ascend/descend):
        // up leg, down leg, up leg.
        let leg = |start: f64, slope: f64, n: usize| -> Vec<f64> {
            (0..n)
                .map(|i| start + slope * i as f64 + 8.0 * (i as f64 * 0.314).sin())
                .collect::<Vec<f64>>()
        };
        let mut closes = leg(100.0, 0.4, 200);
        closes.extend(leg(180.0, -0.4, 200));
        closes.extend(leg(100.0, 0.4, 200));
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        let ups = x_structure_daily_flip_up(&ctx);
        let downs = x_structure_daily_flip_down(&ctx);
        // Each regime flip fires ONCE per transition, not on every bar of
        // the trend (3 legs ≈ 600 bars would otherwise flood the stream).
        assert!(!ups.is_empty(), "expected at least one flip-up");
        assert!(!downs.is_empty(), "expected at least one flip-down");
        assert!(
            ups.len() <= 6 && downs.len() <= 6,
            "flips must be transitions, got {} ups / {} downs",
            ups.len(),
            downs.len()
        );
        // No duplicate dates within a stream.
        let mut dates: Vec<&str> = ups.iter().map(|e| e.date.as_str()).collect();
        dates.dedup();
        assert_eq!(dates.len(), ups.len());
        // Chronological.
        for w in ups.windows(2) {
            assert!(w[0].date < w[1].date);
        }
    }

    #[test]
    fn bos_events_fire_on_level_breaks_and_deactivate() {
        // Build a clear swing low then break it once: rise, dip (swing low),
        // rise (confirms), then collapse through the low.
        let mut closes: Vec<f64> = Vec::new();
        for i in 0..30 {
            closes.push(100.0 + i as f64); // ramp to 129
        }
        for i in 0..10 {
            closes.push(129.0 - (i + 1) as f64 * 2.0); // dip to 109
        }
        for i in 0..20 {
            closes.push(109.0 + (i + 1) as f64 * 2.0); // recover to 149
        }
        for i in 0..20 {
            closes.push(149.0 - (i + 1) as f64 * 3.0); // collapse to 89
        }
        closes.extend(vec![89.0; 15]);
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        let support = x_structure_daily_bos_support(&ctx);
        assert_eq!(
            support.len(),
            1,
            "one support break expected (level deactivates after the break): {support:?}"
        );
        // The break must date AFTER the swing low (index 39) was confirmed.
        assert!(support[0].date.as_str() > history[44].date.as_str());
    }

    #[test]
    fn extension_and_window_onsets_do_not_repeat_while_state_holds() {
        // 250 flat bars then a sharp sustained rally far above the 200dma.
        let mut closes = vec![100.0; 250];
        closes.extend((0..60).map(|i| 130.0 + i as f64));
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        let ext = x_extension_20pct_over_200dma(&ctx);
        assert_eq!(ext.len(), 1, "one extension onset expected: {ext:?}");
        // The window signal: flat tape sits inside the window from the first
        // defined SMA bar (one onset), exits in the rally, never re-enters.
        let win = x_window_within_5pct_200dma(&ctx);
        assert_eq!(win.len(), 1, "one window onset expected: {win:?}");
    }

    #[test]
    fn rsi_oversold_enter_is_a_cross_event() {
        // Rally then a crash producing a deep RSI dip; recovery; second crash.
        let mut closes: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        closes.extend((0..25).map(|i| 160.0 - (i + 1) as f64 * 4.0));
        closes.extend((0..40).map(|i| 60.0 + (i + 1) as f64 * 2.0));
        closes.extend((0..25).map(|i| 140.0 - (i + 1) as f64 * 4.0));
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        let events = x_rsi14_oversold_25_enter(&ctx);
        assert!(
            (1..=2).contains(&events.len()),
            "one onset per oversold episode: {events:?}"
        );
    }

    #[test]
    fn mayer_under_085_onset() {
        // 250 flat bars at 100, then a crash to 80 (mayer ~0.8) held flat.
        let mut closes = vec![100.0; 250];
        closes.extend(vec![80.0; 30]);
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        let events = x_mayer_under_085_enter(&ctx);
        assert_eq!(events.len(), 1, "{events:?}");
    }

    #[test]
    fn cyberline_crosses_emit_both_directions_and_match_last_cross() {
        let closes = wave(400);
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        let ups = x_cyberline_cross_up(&ctx);
        let downs = x_cyberline_cross_down(&ctx);
        assert!(!ups.is_empty() && !downs.is_empty());
        // The newest cross across both streams equals the engine's last_cross.
        let last_engine = ctx
            .cyber
            .as_ref()
            .and_then(|c| c.line.as_ref())
            .and_then(|l| l.last_cross.clone())
            .expect("engine last cross");
        let newest = ups
            .iter()
            .chain(downs.iter())
            .max_by(|a, b| a.date.cmp(&b.date))
            .expect("newest");
        assert_eq!(newest.date, last_engine.date);
    }

    #[test]
    fn context_build_is_deterministic() {
        let closes = wave(600);
        let history = series_from(&closes);
        let a = AssetContext::build("TEST", "TEST", &history).expect("a");
        let b = AssetContext::build("TEST", "TEST", &history).expect("b");
        for def in registry() {
            let ea = serde_json::to_string(&def.emit(&a)).unwrap_or_default();
            let eb = serde_json::to_string(&def.emit(&b)).unwrap_or_default();
            assert_eq!(ea, eb, "emitter {} must be deterministic", def.id);
        }
    }

    #[test]
    fn events_are_chronological_for_every_emitter() {
        let closes = wave(700);
        let history = series_from(&closes);
        let ctx = AssetContext::build("TEST", "TEST", &history).expect("ctx");
        for def in registry() {
            let events = def.emit(&ctx);
            for w in events.windows(2) {
                assert!(
                    w[0].date <= w[1].date,
                    "emitter {} events out of order",
                    def.id
                );
            }
        }
    }
}
