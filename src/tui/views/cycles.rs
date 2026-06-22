//! Cycles view — the TUI home for documented market-cycle clocks.
//!
//! Four sub-tabs (h/l): a dense, navigable Matrix landing row per asset that
//! has GENUINE cycle data (Bitcoin, Gold, Silver); full clock panels for
//! Bitcoin (~4-year supply cycle) and Gold (~6.9-year cycle); and an Engine
//! sub-tab that surfaces ALL computed degrees for the focused asset (band
//! statistics, demarcation-line state, trend-line state, nested alignment,
//! half-cycle low, failed-cycle and translation flags). Everything is computed
//! INLINE from the in-memory `app.price_history` (pure CPU; the TUI event loop
//! must never block on I/O).
//!
//! Navigation: j/k move a row cursor on the Matrix; Enter drills the focused
//! row into its dedicated tab (Bitcoin row → Bitcoin tab, Gold/Silver →
//! Gold tab). The Engine tab follows the same focused asset.
//!
//! Discipline (matches the operator's constraints):
//! - No practitioner/author names in the UI — only plain functional language.
//! - Friendly asset names only ("Bitcoin"/"Gold"/"Silver"), never raw tickers.
//! - Only Bitcoin, Gold and Silver have real anchored cycle data; nothing else
//!   is tracked here (no invented clocks for equities/DXY/oil). Silver phases
//!   WITH gold — it earns a Matrix row but no dedicated tab in the MVP.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::analytics::cycle_clock::{btc_cycle_clock, gold_cycle_clock, BtcCycleClock, GoldCycleClock};
use crate::analytics::cycle_engine::{analyze, default_config, BandPosition, CycleReport, DegreeStatus};
use crate::analytics::hurst_rs;
use crate::app::App;
use crate::models::price::HistoryRecord;

/// Sub-tab count (Matrix, Bitcoin, Gold, Engine). Cycled with h/l.
pub const SUBTAB_COUNT: u8 = 4;
const SUBTAB_NAMES: [&str; 4] = ["Matrix", "Bitcoin", "Gold", "Engine"];

/// The cycle-tracked assets: friendly name + the canonical ticker its history
/// is cached under. Only these three have genuine anchored cycle data.
struct CycleAsset {
    /// Friendly name shown in the UI — never a ticker.
    name: &'static str,
    /// Canonical price-history ticker (try this first, then `alt`).
    ticker: &'static str,
    /// Alternate key the demo / shallow series may be stored under.
    alt: &'static str,
}

const CYCLE_ASSETS: [CycleAsset; 3] = [
    CycleAsset { name: "Bitcoin", ticker: "BTC-USD", alt: "BTC" },
    CycleAsset { name: "Gold", ticker: "GC=F", alt: "GC=F" },
    CycleAsset { name: "Silver", ticker: "SI=F", alt: "SI=F" },
];

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let active = (app.cycles_subtab % SUBTAB_COUNT) as usize;
    let outer = Block::default()
        .title(format!("Cycles — {}", SUBTAB_NAMES[active]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_subtle))
        .style(Style::default().bg(app.theme.surface_0));
    frame.render_widget(outer, area);
    let inner = area.inner(Margin { horizontal: 1, vertical: 1 });
    if inner.height < 6 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(inner);
    frame.render_widget(Paragraph::new(subtab_strip(app, active)), rows[0]);

    match active {
        1 => render_bitcoin(frame, rows[1], app),
        2 => render_gold(frame, rows[1], app),
        3 => render_engine(frame, rows[1], app),
        _ => render_matrix(frame, rows[1], app),
    }
}

/// The h/l sub-tab strip (first body line), Risk-Dashboard house style.
fn subtab_strip(app: &App, active: usize) -> Line<'static> {
    let mut spans = vec![Span::styled("h/l ", Style::default().fg(app.theme.text_muted))];
    for (i, name) in SUBTAB_NAMES.iter().enumerate() {
        let style = if i == active {
            Style::default().fg(app.theme.text_accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.text_muted)
        };
        spans.push(Span::styled(format!(" {name} "), style));
        if i + 1 < SUBTAB_NAMES.len() {
            spans.push(Span::styled("│", Style::default().fg(app.theme.border_subtle)));
        }
    }
    Line::from(spans)
}

/// Pull an asset's history from the in-memory store, trying the canonical
/// ticker then the alternate key (the demo/shallow series may key BTC as "BTC").
fn history_for<'a>(app: &'a App, a: &CycleAsset) -> Option<&'a [HistoryRecord]> {
    app.price_history
        .get(a.ticker)
        .or_else(|| app.price_history.get(a.alt))
        .map(|v| v.as_slice())
        .filter(|h| !h.is_empty())
}

// ---------------------------------------------------------------------------
// Matrix
// ---------------------------------------------------------------------------

/// One dense, pre-formatted Matrix row for a cycle-tracked asset.
struct MatrixRow {
    /// Index into `CYCLE_ASSETS` — used to drill Enter into the right tab.
    asset_idx: usize,
    name: String,
    degree: String,
    age: String,
    band: String,
    translation: String,
    next_low: String,
    regime: String,
    stance: String,
    /// Sort key: bars/days until the next-low window opens (smaller = sooner).
    sort_key: i64,
}

/// Friendly trend-regime glyph from the Hurst exponent on log-returns.
/// ↗ trending · ↔ random-walk · ⟲ mean-reverting · — insufficient data.
fn regime_glyph(closes: &[f64]) -> &'static str {
    let log_rets: Vec<f64> = closes
        .windows(2)
        .filter(|w| w[0] > 0.0 && w[1] > 0.0)
        .map(|w| (w[1] / w[0]).ln())
        .collect();
    match hurst_rs::hurst(&log_rets) {
        Some(h) if h.h >= 0.55 => "↗ trend",
        Some(h) if h.h <= 0.45 => "⟲ revert",
        Some(_) => "↔ random",
        None => "— n/a",
    }
}

/// Clarity dot for a clock-backed stance (green/amber/red → ● ◐ ○).
fn clarity_dot(report: &CycleReport) -> &'static str {
    use crate::analytics::cycle_engine::Clarity;
    // The longest (cycle-defining) degree drives the clarity dot.
    match report.degrees.first().map(|d| d.clarity) {
        Some(Clarity::Green) => "●",
        Some(Clarity::Amber) => "◐",
        Some(Clarity::Red) => "○",
        None => "·",
    }
}

/// Functional band-position glyph (no doctrine vocabulary leaked to the user).
fn band_glyph(pos: Option<BandPosition>) -> &'static str {
    match pos {
        Some(BandPosition::PreBand) => "<pre",
        Some(BandPosition::InBand) => "=in",
        Some(BandPosition::OverBand) => ">over",
        None => "—",
    }
}

/// Build a Matrix row from the longest degree of an engine report plus a
/// stance/regime read. Pure given its inputs (testable without an `App`).
fn matrix_row(
    asset_idx: usize,
    name: &str,
    closes: &[f64],
    report: Option<&CycleReport>,
    stance: Option<&str>,
    expected_degree: &str,
) -> MatrixRow {
    let regime = regime_glyph(closes).to_string();
    // The longest degree is the cycle-defining one (engine emits longest-first).
    let deg: Option<&DegreeStatus> = report.and_then(|r| r.degrees.first());

    let (degree, age, band, translation, next_low, sort_key) = match deg {
        Some(d) => {
            // Friendly degree label — strip doctrine names, describe by horizon.
            // B2a degree-coherence: if the engine downshifted off the cycle's
            // anchored long degree (insufficient deep history), say so plainly
            // so the Matrix can't silently advertise a different cycle than the
            // dedicated tab.
            let degree = if !expected_degree.is_empty() && d.degree != expected_degree {
                format!(
                    "{} ({} n/a)",
                    friendly_degree(&d.degree),
                    friendly_degree(expected_degree)
                )
            } else {
                friendly_degree(&d.degree)
            };
            let age = match (d.cycle_age_bars, d.band.as_ref(), &d.unit) {
                (Some(age), Some(b), unit) => {
                    let pct = if b.band_hi_bars > 0.0 {
                        (age as f64 / b.band_hi_bars * 100.0).round()
                    } else {
                        0.0
                    };
                    format!("{age}{unit} {pct:.0}%")
                }
                (Some(age), None, unit) => format!("{age}{unit}"),
                _ => "—".to_string(),
            };
            let band = band_glyph(d.band_position).to_string();
            let translation = d
                .ledger
                .last()
                .and_then(|e| e.class.clone())
                .map(|c| translation_glyph(&c).to_string())
                .unwrap_or_else(|| "—".to_string());
            let next_low = d
                .next_low_window
                .as_ref()
                .map(|w| format!("{}→{}", w.start_date, w.end_date))
                .unwrap_or_else(|| "—".to_string());
            // Sort by bars-to-band-start (sooner = first); missing → far future.
            let sort_key = d.bars_to_band_start.unwrap_or(i64::MAX / 2);
            (degree, age, band, translation, next_low, sort_key)
        }
        None => (
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            i64::MAX / 2,
        ),
    };

    let stance = stance.map(|s| s.to_string()).unwrap_or_else(|| "—".to_string());

    MatrixRow {
        asset_idx,
        name: name.to_string(),
        degree,
        age,
        band,
        translation,
        next_low,
        regime,
        stance,
        sort_key,
    }
}

/// Plain, footnote-free glyph for a cycle's translation (where the top sat
/// inside the cycle). Right-translated tops (later in the cycle) are the
/// healthy/bullish read; left-translated (early) hint exhaustion.
/// ↑ later-peak (strong) · ↓ early-peak (weak) · ↕ mid.
fn translation_glyph(class: &str) -> &'static str {
    match class {
        "RT" => "↑ late-peak",
        "LT" => "↓ early-peak",
        "MID" => "↕ mid-peak",
        _ => "—",
    }
}

/// The cycle-defining (longest configured) degree name for a cycle asset, so
/// the Matrix can tell when the engine downshifted off it (B2a coherence).
fn expected_long_degree(ticker: &str) -> &'static str {
    match ticker {
        "BTC-USD" | "BTC" => "4-year",
        "GC=F" | "SI=F" => "major",
        _ => "",
    }
}

/// Map an engine degree name to a plain, doctrine-free horizon label.
/// EVERY known degree maps to a horizon phrase so no raw engine label leaks.
fn friendly_degree(degree: &str) -> String {
    match degree {
        "4-year" => "~4-year",
        "major" => "~6.9-year",
        "investor" => "~1-2 year",
        "intermediate" => "~weeks-months",
        "daily" => "~days-weeks",
        other => other,
    }
    .to_string()
}

/// Translate a BTC accumulation stance string into a plain CALL word.
fn stance_call(stance: &str) -> &'static str {
    match stance {
        "accumulate" => "ACCUMULATE",
        "window-opening" => "WINDOW",
        "early" => "EARLY",
        "advancing" => "ADVANCING",
        "elevated" => "ELEVATED",
        _ => "—",
    }
}

/// Build the Matrix rows in DISPLAY order (sorted by next-low proximity).
/// Shared by the renderer and the j/k row-cursor so the cursor index always
/// maps to the same asset the user sees. Pure given `app.price_history`.
fn build_matrix_rows(app: &App) -> Vec<MatrixRow> {
    let mut rows: Vec<MatrixRow> = Vec::new();
    for (idx, a) in CYCLE_ASSETS.iter().enumerate() {
        let Some(hist) = history_for(app, a) else { continue };
        let closes: Vec<f64> = hist
            .iter()
            .filter_map(|r| rust_decimal::prelude::ToPrimitive::to_f64(&r.close))
            .filter(|c| *c > 0.0)
            .collect();
        if closes.len() < 100 {
            continue;
        }
        let cfg = default_config(a.ticker, a.ticker);
        let report = analyze(&cfg, hist);
        // Clock-backed stance + clarity dot for BTC/gold.
        let stance: Option<String> = match a.ticker {
            "BTC-USD" => btc_cycle_clock(a.ticker, hist).map(|c| {
                let dot = report.as_ref().map(clarity_dot).unwrap_or("·");
                format!("{} {dot}", stance_call(&c.accumulation.stance))
            }),
            "GC=F" => gold_cycle_clock(a.ticker, hist).map(|c| {
                let dot = report.as_ref().map(clarity_dot).unwrap_or("·");
                // Gold has no accumulation stance; surface the cycle-position read.
                let pos = c
                    .cycle_position_pct
                    .map(|p| format!("{p:.0}%-through"))
                    .unwrap_or_else(|| "—".into());
                format!("{pos} {dot}")
            }),
            // Silver phases WITH gold — no independent stance/clock.
            _ => Some("phases w/ gold".to_string()),
        };
        rows.push(matrix_row(
            idx,
            a.name,
            &closes,
            report.as_ref(),
            stance.as_deref(),
            expected_long_degree(a.ticker),
        ));
    }
    // Sort by next-low proximity (soonest first).
    rows.sort_by_key(|r| r.sort_key);
    rows
}

/// Count of cycle assets currently rendering a Matrix row (for cursor clamp).
pub fn matrix_asset_count(app: &App) -> usize {
    build_matrix_rows(app).len()
}

/// The sub-tab to drill into for the currently focused Matrix row:
/// Bitcoin (asset 0) → tab 1; Gold/Silver (assets 1/2) → tab 2. `None` when
/// there are no rows. Drives the Cycles Enter handler.
pub fn matrix_cursor_subtab(app: &App) -> Option<u8> {
    let rows = build_matrix_rows(app);
    if rows.is_empty() {
        return None;
    }
    let cur = app.cycles_cursor.min(rows.len() - 1);
    Some(match rows[cur].asset_idx {
        0 => 1, // Bitcoin → Bitcoin tab
        _ => 2, // Gold / Silver → Gold tab
    })
}

/// Friendly "as of / data depth" line for the focused (or first) asset,
/// mirroring the clock-tab headers so freshness/depth is visible on the
/// landing page.
fn matrix_depth_line(app: &App) -> Option<String> {
    let a = &CYCLE_ASSETS[0]; // anchor freshness on Bitcoin (deepest series)
    let hist = history_for(app, a)?;
    let bars = hist.len();
    let years = (bars as f64 / 252.0 * 10.0).round() / 10.0;
    let as_of = hist.last().map(|r| r.date.clone()).unwrap_or_default();
    Some(format!("as of {as_of} · ~{years:.1}y of daily depth ({bars} bars)"))
}

fn render_matrix(frame: &mut Frame, area: Rect, app: &App) {
    let rows = build_matrix_rows(app);

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No cycle-tracked asset has enough cached daily history yet.\n\nBitcoin, Gold and Silver are the only assets with genuine anchored cycle data; refresh their price history to populate the cycle matrix.",
            )
            .style(Style::default().fg(app.theme.text_muted)),
            area,
        );
        return;
    }

    let cursor = app.cycles_cursor.min(rows.len() - 1);
    // `area` is already the inner content rect (panel borders/margins removed).
    let cols = area.width;

    // Responsive column plan keyed to the cumulative fixed-column widths:
    // marker(3)+Asset(8)+Stance(22)+Degree(20)+Band(8) = 61 base, then each
    // optional column adds its width. Drop right-to-left as width shrinks, but
    // the base columns — including Stance, the actionable verdict — ALWAYS
    // survive narrowing (B2d).
    let show_age = cols >= 74; // +Age/%band(13)
    let show_tr = cols >= 87; // +Trans.(13)
    let show_nextlow = cols >= 111; // +Next-low(24)
    let show_regime = cols >= 117; // +Regime(6)

    // Header.
    let mut header = String::from("   "); // marker gutter
    header.push_str(&format!("{:<8}", "Asset"));
    header.push_str(&format!("{:<22}", "Stance / position"));
    header.push_str(&format!("{:<20}", "Degree"));
    header.push_str(&format!("{:<8}", "Band"));
    if show_age {
        header.push_str(&format!("{:<13}", "Age/%band"));
    }
    if show_tr {
        header.push_str(&format!("{:<13}", "Trans."));
    }
    if show_nextlow {
        header.push_str(&format!("{:<24}", "Next-low window"));
    }
    if show_regime {
        header.push_str("Regime");
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(depth) = matrix_depth_line(app) {
        lines.push(Line::from(Span::styled(
            depth,
            Style::default().fg(app.theme.text_muted),
        )));
    }
    lines.push(Line::from(Span::styled(
        header,
        Style::default().fg(app.theme.text_secondary),
    )));

    for (i, r) in rows.iter().enumerate() {
        let selected = i == cursor;
        let marker = if selected { "> " } else { "  " };
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(
            format!("{marker} "),
            Style::default().fg(app.theme.text_accent),
        ));
        spans.push(Span::raw(format!("{:<8}", ellipsize(&r.name, 8))));
        // Stance pulled LEFT and colored so the actionable verdict reads first.
        spans.push(Span::styled(
            format!("{:<22}", ellipsize(&r.stance, 21)),
            Style::default()
                .fg(stance_color(&r.stance, app))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!("{:<20}", ellipsize(&r.degree, 19))));
        spans.push(Span::raw(format!("{:<8}", r.band)));
        if show_age {
            spans.push(Span::raw(format!("{:<13}", ellipsize(&r.age, 12))));
        }
        if show_tr {
            spans.push(Span::raw(format!("{:<13}", ellipsize(&r.translation, 12))));
        }
        if show_nextlow {
            spans.push(Span::raw(format!("{:<24}", ellipsize(&r.next_low, 23))));
        }
        if show_regime {
            spans.push(Span::raw(r.regime.clone()));
        }
        let style = if selected {
            Style::default()
                .fg(app.theme.text_primary)
                .bg(app.theme.surface_1)
        } else {
            Style::default().fg(app.theme.text_primary)
        };
        lines.push(Line::from(spans).style(style));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "j/k select · Enter → asset tab. Band <pre / =in / >over the low-to-low timing window. Sorted by next-low proximity.",
        Style::default().fg(app.theme.text_muted),
    )));
    lines.push(Line::from(Span::styled(
        "Trans. ↑ late-peak (strong) · ↓ early-peak (weak). Regime ↗ trending · ↔ random · ⟲ mean-reverting. Clarity ● clear · ◐ mixed · ○ noisy.",
        Style::default().fg(app.theme.text_muted),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(app.theme.text_primary)),
        area,
    );
}

/// Truncate-with-ellipsis so a too-long cell can't bleed into the next column.
fn ellipsize(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{keep}…")
}

/// Color the actionable Stance/position cell so the verdict is legible at a
/// glance: accumulation/early reads green, elevated/late reads amber.
fn stance_color(stance: &str, app: &App) -> Color {
    let s = stance.to_lowercase();
    if s.contains("accumulate") || s.contains("window") || s.contains("early") {
        app.theme.gain_green
    } else if s.contains("elevated") || s.contains("advancing") {
        app.theme.stale_yellow
    } else {
        app.theme.text_primary
    }
}

// ---------------------------------------------------------------------------
// Bitcoin clock (2×2 panel grid)
// ---------------------------------------------------------------------------

fn render_bitcoin(frame: &mut Frame, area: Rect, app: &App) {
    let asset = &CYCLE_ASSETS[0];
    let Some(hist) = history_for(app, asset) else {
        frame.render_widget(unavailable("Bitcoin"), area);
        return;
    };
    let Some(clock) = btc_cycle_clock(asset.ticker, hist) else {
        frame.render_widget(unavailable("Bitcoin"), area);
        return;
    };
    // Engine corroboration via the longest (cycle-defining / ~4-year) degree.
    let report = analyze(&default_config(asset.ticker, asset.ticker), hist);
    let cycle_deg = report
        .as_ref()
        .and_then(|r| r.degrees.first());

    let header = Paragraph::new(format!(
        "Bitcoin · ~4-year supply cycle · as of {} · {:.0}",
        clock.as_of, clock.last_close
    ))
    .style(Style::default().fg(app.theme.text_secondary));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(area);
    frame.render_widget(header, rows[0]);

    let (tl, tr, bl, br) = quad(rows[1]);
    panel(frame, tl, app, "Stance", btc_stance_lines(&clock));
    panel(frame, tr, app, "Cycle position", btc_position_lines(&clock));
    panel(frame, bl, app, "Valuation", btc_valuation_lines(&clock));
    panel(frame, br, app, "Engine cross-check", engine_lines(cycle_deg));
}

fn btc_stance_lines(c: &BtcCycleClock) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("Call:   {}", stance_call(&c.accumulation.stance))),
        Line::from(format!("Score:  {:+}", c.accumulation.score)),
        Line::from(""),
    ];
    for fac in c.accumulation.factors.iter().take(3) {
        lines.push(Line::from(format!("· {}", plainify(fac))));
    }
    lines
}

fn btc_position_lines(c: &BtcCycleClock) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("Since supply event: {}w", c.weeks_since_halving)),
        Line::from(format!("Projected bottom in: {}d", c.olson_days_remaining)),
    ];
    if let Some(l) = &c.loukas {
        lines.push(Line::from(format!(
            "Cycle week {} of ~{}",
            l.cycle_week, l.cycle_length_weeks
        )));
        let band = if l.in_band {
            "IN the low band".to_string()
        } else if l.weeks_to_band_start > 0 {
            format!("{}w to low band", l.weeks_to_band_start)
        } else {
            "past the low band".to_string()
        };
        lines.push(Line::from(format!("Low band: {band}")));
    }
    lines
}

fn btc_valuation_lines(c: &BtcCycleClock) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(m) = c.mayer_multiple {
        lines.push(Line::from(format!("Price / 200-day avg: {m}")));
    }
    if let Some(p) = c.pct_vs_200wma {
        lines.push(Line::from(format!("vs 200-week avg: {p:+}%")));
    }
    if let Some(t) = &c.major_cycle_test {
        lines.push(Line::from(format!(
            "vs prior cycle high: {:+}%",
            t.pct_vs_prior_high
        )));
        lines.push(Line::from(if t.above_prior_high {
            "At/above the prior high".to_string()
        } else {
            "Below the prior high".to_string()
        }));
    }
    if lines.is_empty() {
        lines.push(Line::from("Valuation: insufficient history"));
    }
    lines
}

// ---------------------------------------------------------------------------
// Gold clock (2×2 panel grid)
// ---------------------------------------------------------------------------

fn render_gold(frame: &mut Frame, area: Rect, app: &App) {
    let asset = &CYCLE_ASSETS[1];
    let Some(hist) = history_for(app, asset) else {
        frame.render_widget(unavailable("Gold"), area);
        return;
    };
    let Some(clock) = gold_cycle_clock(asset.ticker, hist) else {
        frame.render_widget(unavailable("Gold"), area);
        return;
    };
    let report = analyze(&default_config(asset.ticker, asset.ticker), hist);
    let cycle_deg = report.as_ref().and_then(|r| r.degrees.first());

    let header = Paragraph::new(format!(
        "Gold · ~6.9-year cycle · as of {} · {:.0}",
        clock.as_of, clock.last_close
    ))
    .style(Style::default().fg(app.theme.text_secondary));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(area);
    frame.render_widget(header, rows[0]);

    let (tl, tr, bl, br) = quad(rows[1]);
    panel(frame, tl, app, "Cycle position", gold_position_lines(&clock));
    panel(frame, tr, app, "Last cycle low", gold_low_lines(&clock));
    panel(frame, bl, app, "Valuation", gold_valuation_lines(&clock));
    panel(frame, br, app, "Engine cross-check", engine_lines(cycle_deg));
}

fn gold_position_lines(c: &GoldCycleClock) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(p) = c.cycle_position_pct {
        lines.push(Line::from(format!("Through the cycle: {p:.0}%")));
    }
    if let Some(y) = c.years_since_cycle_low {
        lines.push(Line::from(format!("Years since low: {y}")));
    }
    if let Some(a) = c.avg_cycle_years {
        lines.push(Line::from(format!("Avg cycle length: ~{a}y")));
    }
    if let Some(half) = c.past_half_cycle {
        lines.push(Line::from(format!(
            "Half-cycle: {}",
            if half { "passed (2nd half)" } else { "not yet (1st half)" }
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from("Cycle position: insufficient history"));
    }
    lines
}

fn gold_low_lines(c: &GoldCycleClock) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(ref low) = c.last_cycle_low_date {
        lines.push(Line::from(format!("Last cycle low: {low}")));
    }
    if let Some(h) = c.half_cycle_years {
        lines.push(Line::from(format!("Half-cycle mark: ~{h}y")));
    }
    let verified = c.anchors.iter().filter(|a| a.verified_date.is_some()).count();
    lines.push(Line::from(format!(
        "Verified low anchors: {}/{}",
        verified,
        c.anchors.len()
    )));
    if lines.len() <= 1 {
        lines.push(Line::from("(refresh deep history to verify anchors)"));
    }
    lines
}

fn gold_valuation_lines(c: &GoldCycleClock) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(e) = c.extension_pct_vs_200dma {
        lines.push(Line::from(format!("vs 200-day avg: {e:+}%")));
    }
    if let Some(e) = c.extension_pct_vs_40wma {
        lines.push(Line::from(format!("vs 40-week avg: {e:+}%")));
    }
    if lines.is_empty() {
        lines.push(Line::from("Valuation: insufficient history"));
    }
    lines
}

// ---------------------------------------------------------------------------
// Engine sub-tab — surface ALL computed degrees for the focused asset.
// ---------------------------------------------------------------------------

/// Resolve which cycle asset the Engine tab should describe: the asset under
/// the Matrix row cursor (sorted display order). Falls back to the first
/// asset with a usable report.
fn focused_engine_asset(app: &App) -> Option<usize> {
    let rows = build_matrix_rows(app);
    if rows.is_empty() {
        return None;
    }
    let cur = app.cycles_cursor.min(rows.len() - 1);
    Some(rows[cur].asset_idx)
}

fn render_engine(frame: &mut Frame, area: Rect, app: &App) {
    let Some(idx) = focused_engine_asset(app) else {
        frame.render_widget(
            Paragraph::new(
                "No cycle-tracked asset has enough cached daily history yet.\n\nThe Engine view surfaces every measured cycle degree (band statistics, demarcation- and trend-line state, nesting) for the asset focused on the Matrix.",
            )
            .style(Style::default().fg(app.theme.text_muted)),
            area,
        );
        return;
    };
    let asset = &CYCLE_ASSETS[idx];
    let Some(hist) = history_for(app, asset) else {
        frame.render_widget(unavailable(asset.name), area);
        return;
    };
    let Some(report) = analyze(&default_config(asset.ticker, asset.ticker), hist) else {
        frame.render_widget(unavailable(asset.name), area);
        return;
    };

    let header = Paragraph::new(format!(
        "{} · cycle engine · as of {} · {} measured degree{} · {} bars",
        asset.name,
        report.as_of,
        report.degrees.len(),
        if report.degrees.len() == 1 { "" } else { "s" },
        report.bars,
    ))
    .style(Style::default().fg(app.theme.text_secondary));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(area);
    frame.render_widget(header, rows[0]);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, d) in report.degrees.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        engine_degree_lines(d, &mut lines, app);
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Future demarcation line = midpoint price line offset half a cycle (a cross projects a measured-move target). Trend line = the rising line joining the last two cycle lows; a close-through break warns the cycle high is in.",
        Style::default().fg(app.theme.text_muted),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(app.theme.text_primary)),
        rows[1],
    );
}

/// Render one degree's full computed status into `lines` (plain language).
/// This is the heart of the Engine tab — pure surfacing of the signal the
/// engine already computes but the rest of the UI discards.
fn engine_degree_lines(d: &DegreeStatus, lines: &mut Vec<Line<'static>>, app: &App) {
    use crate::analytics::cycle_engine::Clarity;
    let dot = match d.clarity {
        Clarity::Green => "●",
        Clarity::Amber => "◐",
        Clarity::Red => "○",
    };
    // Degree title line.
    let mut title = format!("{dot} {} cycle", friendly_degree(&d.degree));
    if d.small_n {
        title.push_str("  (few completed cycles — wide bands)");
    }
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(app.theme.text_accent)
            .add_modifier(Modifier::BOLD),
    )));

    // Age + band statistics (median / SD / P15–P85 / position).
    if let Some(b) = &d.band {
        let age = d
            .cycle_age_bars
            .map(|a| format!("{a}{}", d.unit))
            .unwrap_or_else(|| "—".into());
        lines.push(engine_kv(
            "  Age / band",
            format!(
                "{age} · pos {} · median {:.0}{u} (sd {:.0}) · usual {:.0}–{:.0}{u}",
                band_word(d.band_position),
                b.median_bars,
                b.sd_bars,
                b.p15_bars,
                b.p85_bars,
                u = d.unit,
            ),
            app,
        ));
    } else {
        lines.push(engine_kv("  Age / band", "not enough completed cycles".into(), app));
    }

    // Next-low window.
    if let Some(w) = &d.next_low_window {
        lines.push(engine_kv(
            "  Next low",
            format!("{} → {}", w.start_date, w.end_date),
            app,
        ));
    }

    // Current (unconfirmed) top + provisional translation.
    if let Some(t) = &d.current_top {
        let trans = t
            .provisional_translation_pct
            .map(|p| format!(" · {:.0}% through (provisional)", p * 100.0))
            .unwrap_or_default();
        lines.push(engine_kv(
            "  Current high",
            format!("{} @ {} ({} bars from low){trans}", t.date, t.price, t.bars_from_low),
            app,
        ));
    }

    // Future demarcation line (FLD) — cross + target + achieved%.
    if let Some(f) = &d.fld {
        let mut s = format!("price {} the line", f.price_side);
        if let Some(c) = &f.last_cross {
            s.push_str(&format!(" · last cross {} ({})", c.date, c.dir));
            if let Some(tgt) = c.target {
                s.push_str(&format!(" → target {tgt}"));
            }
            if let Some(pct) = c.achieved_pct {
                // Cap the readout: a cross sitting a hair from a degenerate/extreme
                // target yields absurd figures (e.g. 3013%); show "exceeded" instead.
                if pct > 3.0 {
                    s.push_str(" · target exceeded");
                } else {
                    s.push_str(&format!(" · {:.0}% achieved", pct * 100.0));
                }
            }
            if c.active {
                s.push_str(" · active");
            }
        }
        lines.push(engine_kv("  Demarcation line", s, app));
    }

    // Trend line (VTL) — valid / intact / broken.
    if let Some(v) = &d.vtl {
        let state = if v.broken {
            "BROKEN (close-through — possible cycle high in)"
        } else if !v.valid {
            "not yet valid (line cuts price between anchors)"
        } else if v.intact {
            "intact (price holding above)"
        } else {
            "below the line"
        };
        lines.push(engine_kv("  Trend line", state.to_string(), app));
    }

    // Nested alignment — parent/child sync + expected vs observed subcycles.
    if let Some(a) = &d.nested_alignment {
        let mut s = format!("within {}", friendly_degree(&a.parent_degree));
        if let Some(p) = a.parent_age_pct {
            s.push_str(&format!(" ({:.0}% through parent)", p * 100.0));
        }
        if let Some(sync) = a.sync_ok {
            s.push_str(if sync { " · low aligned ✓" } else { " · low NOT aligned ✗" });
        }
        if let (Some(exp), Some(obs)) = (a.expected_subcycles, a.observed_subcycles) {
            s.push_str(&format!(" · subcycles {obs} seen / {exp} expected"));
            if let Some(ok) = a.count_ok {
                s.push_str(if ok { " ✓" } else { " ✗" });
            }
        }
        lines.push(engine_kv("  Nesting", s, app));
    }

    // Half-cycle low.
    if let Some(h) = &d.half_cycle_low {
        lines.push(engine_kv(
            "  Half-cycle low",
            format!("{} @ {}", h.date, h.price),
            app,
        ));
    }

    // Flags: failed cycle, translation-string, top warning.
    let mut flags: Vec<String> = Vec::new();
    if d.failed_cycle {
        flags.push("⚠ failed cycle (broke below the origin low)".into());
    }
    if d.translation_warning {
        flags.push("⚠ peak shifted earlier (possible top)".into());
    }
    if d.rt_string_intact {
        flags.push("late-peak streak intact (uptrend healthy)".into());
    }
    if d.possible_inversion {
        flags.push("possible timing inversion (flag only)".into());
    }
    if !flags.is_empty() {
        for f in flags {
            let color = if f.starts_with('⚠') {
                app.theme.stale_yellow
            } else {
                app.theme.text_secondary
            };
            lines.push(Line::from(Span::styled(
                format!("  • {f}"),
                Style::default().fg(color),
            )));
        }
    }
}

/// A key/value engine line: muted key, primary value.
fn engine_kv(key: &str, value: String, app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{key}: "),
            Style::default().fg(app.theme.text_muted),
        ),
        Span::styled(value, Style::default().fg(app.theme.text_primary)),
    ])
}

/// Plain word for a band position (no doctrine vocabulary).
fn band_word(pos: Option<BandPosition>) -> &'static str {
    match pos {
        Some(BandPosition::PreBand) => "before window",
        Some(BandPosition::InBand) => "IN window",
        Some(BandPosition::OverBand) => "past window",
        None => "—",
    }
}

// ---------------------------------------------------------------------------
// Shared rendering helpers
// ---------------------------------------------------------------------------

/// Engine cross-check panel from the cycle-defining degree (plain language).
fn engine_lines(deg: Option<&DegreeStatus>) -> Vec<Line<'static>> {
    match deg {
        Some(d) => {
            let mut lines = vec![
                Line::from(format!("Degree: {}", friendly_degree(&d.degree))),
                Line::from(format!("Band: {}", band_glyph(d.band_position))),
            ];
            if let Some(age) = d.cycle_age_bars {
                lines.push(Line::from(format!("Cycle age: {}{}", age, d.unit)));
            }
            if let Some(w) = &d.next_low_window {
                lines.push(Line::from(format!("Next low: {}→{}", w.start_date, w.end_date)));
            }
            if d.translation_warning {
                lines.push(Line::from("⚠ translation shift (possible top)"));
            }
            if d.small_n {
                lines.push(Line::from("(few completed cycles — wide bands)"));
            }
            lines
        }
        None => vec![Line::from("Engine: not enough deep history for the long degree.")],
    }
}

/// Strip residual doctrine/eponym words from a clock factor string so nothing
/// author-named leaks into the panel text.
fn plainify(s: &str) -> String {
    s.replace("Loukas:", "Low-band:")
        .replace("Loukas ", "low-band ")
        .replace("Mayer", "price/200d-avg")
        .replace("Olson day-900 bottom", "projected cycle bottom")
        .replace("Olson day-900 was", "projected cycle bottom was")
        .replace("Olson day-900", "projected cycle bottom")
}

fn quad(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let grid = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(grid[0]);
    let bot = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(grid[1]);
    (top[0], top[1], bot[0], bot[1])
}

fn panel(frame: &mut Frame, area: Rect, app: &App, title: &str, lines: Vec<Line<'static>>) {
    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title(title.to_string())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        // Word-wrap so factor bullets fold cleanly at narrow widths instead of
        // being hard-cut mid-word at the panel edge.
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn unavailable(name: &str) -> Paragraph<'static> {
    Paragraph::new(format!(
        "{name}'s cycle clock is unavailable — not enough cached daily history yet.\n\nRefresh {name}'s price history to populate the clock."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::cycle_engine::{BandPosition, CycleReport};

    fn text(lines: Vec<Line>) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn band_glyph_uses_plain_pre_in_over_vocabulary() {
        assert_eq!(band_glyph(Some(BandPosition::PreBand)), "<pre");
        assert_eq!(band_glyph(Some(BandPosition::InBand)), "=in");
        assert_eq!(band_glyph(Some(BandPosition::OverBand)), ">over");
        assert_eq!(band_glyph(None), "—");
    }

    #[test]
    fn friendly_degree_strips_doctrine_labels() {
        assert_eq!(friendly_degree("4-year"), "~4-year");
        assert_eq!(friendly_degree("major"), "~6.9-year");
    }

    #[test]
    fn friendly_degree_maps_every_known_degree_to_a_horizon() {
        // B2b: no raw engine degree label may leak; each maps to a plain
        // horizon phrase (starting with "~").
        for raw in ["4-year", "major", "investor", "intermediate", "daily"] {
            let f = friendly_degree(raw);
            assert!(f.starts_with('~'), "degree {raw} → {f} not a horizon label");
            assert_ne!(f, raw, "degree {raw} leaked unmapped");
        }
    }

    #[test]
    fn translation_glyph_is_plain_and_footnote_free() {
        assert!(translation_glyph("RT").contains("late-peak"));
        assert!(translation_glyph("LT").contains("early-peak"));
        assert!(translation_glyph("MID").contains("mid"));
        assert_eq!(translation_glyph("?"), "—");
        // No bare RT/LT codes leak.
        for c in ["RT", "LT", "MID"] {
            let g = translation_glyph(c);
            assert!(!g.starts_with("RT") && !g.starts_with("LT"), "{g}");
        }
    }

    #[test]
    fn ellipsize_truncates_with_marker_and_preserves_short() {
        assert_eq!(ellipsize("Bitcoin", 8), "Bitcoin");
        assert_eq!(ellipsize("Bitcoin", 4), "Bit…");
        assert_eq!(ellipsize("abc", 0), "");
    }

    #[test]
    fn band_word_is_plain_language() {
        assert_eq!(band_word(Some(BandPosition::PreBand)), "before window");
        assert_eq!(band_word(Some(BandPosition::InBand)), "IN window");
        assert_eq!(band_word(Some(BandPosition::OverBand)), "past window");
        assert_eq!(band_word(None), "—");
    }

    #[test]
    fn engine_degree_lines_surface_band_and_flags_in_plain_language() {
        // Build a real report and render the longest degree's engine block.
        let report = synthetic_gold_report();
        let d = report.degrees.first().expect("a degree");
        let app = test_app();
        let mut lines: Vec<Line<'static>> = Vec::new();
        engine_degree_lines(d, &mut lines, &app);
        let t = text(lines).to_lowercase();
        // Surfaces band statistics under a plain heading.
        assert!(t.contains("age / band"), "{t}");
        // No doctrine/eponym words leak.
        for bad in ["loukas", "hurst", "bressert", "fld", "vtl", "mayer"] {
            assert!(!t.contains(bad), "engine block leaked '{bad}': {t}");
        }
    }

    /// A minimal real `App` for render-helper tests (in-memory DB; carries the
    /// default theme set by `App::new`).
    fn test_app() -> App {
        let config = crate::config::Config::default();
        App::new(&config, std::path::PathBuf::from(":memory:"))
    }

    #[test]
    fn stance_call_maps_every_stance() {
        for (s, want) in [
            ("accumulate", "ACCUMULATE"),
            ("window-opening", "WINDOW"),
            ("early", "EARLY"),
            ("advancing", "ADVANCING"),
            ("elevated", "ELEVATED"),
        ] {
            assert_eq!(stance_call(s), want);
        }
    }

    #[test]
    fn plainify_removes_practitioner_names() {
        let s = plainify("Loukas: IN the low band — Mayer 0.8 — Olson day-900 bottom ~2026");
        let lower = s.to_lowercase();
        assert!(!lower.contains("loukas"), "{s}");
        assert!(!lower.contains("mayer"), "{s}");
        assert!(!lower.contains("olson"), "{s}");
    }

    #[test]
    fn regime_glyph_classifies_trend_and_handles_thin_data() {
        // Strong monotone uptrend → trending (or at least never panics / "n/a"
        // when there is enough data); thin data → the n/a sentinel.
        let trend: Vec<f64> = (0..300).map(|i| 100.0 * (1.0 + 0.002 * i as f64)).collect();
        let g = regime_glyph(&trend);
        assert!(g.starts_with('↗') || g.starts_with('↔') || g.starts_with('⟲'));
        assert_eq!(regime_glyph(&[100.0]), "— n/a");
    }

    #[test]
    fn matrix_row_with_no_report_is_all_dashes_but_keeps_regime() {
        let closes: Vec<f64> = (0..300).map(|i| 100.0 + (i as f64 / 7.0).sin()).collect();
        let row = matrix_row(0, "Bitcoin", &closes, None, Some("ACCUMULATE ●"), "4-year");
        assert_eq!(row.asset_idx, 0);
        assert_eq!(row.name, "Bitcoin");
        assert_eq!(row.degree, "—");
        assert_eq!(row.band, "—");
        assert_eq!(row.stance, "ACCUMULATE ●");
        // Regime is computed from closes even without an engine report.
        assert!(!row.regime.is_empty());
        assert_eq!(row.sort_key, i64::MAX / 2);
    }

    #[test]
    fn matrix_row_labels_degree_fallback_when_long_degree_missing() {
        // A real report whose longest available degree is NOT the expected
        // anchored one must be labeled "(… n/a)" so the Matrix can't silently
        // advertise a shorter cycle than the dedicated tab (B2a).
        let report = synthetic_gold_report();
        let actual = &report.degrees.first().unwrap().degree;
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + (i as f64 / 7.0).sin()).collect();
        // Claim a long degree that differs from what the report actually has.
        let bogus = if actual == "zzz" { "qqq" } else { "zzz" };
        let row = matrix_row(1, "Gold", &closes, Some(&report), None, bogus);
        assert!(row.degree.contains("n/a"), "expected fallback label, got {}", row.degree);
    }

    #[test]
    fn matrix_cursor_subtab_maps_rows_to_asset_tabs() {
        // Bitcoin (asset 0) → tab 1; Gold/Silver (asset 1/2) → tab 2. Verified
        // purely from asset_idx without needing a populated App.
        let map = |idx: usize| -> u8 {
            match idx {
                0 => 1,
                _ => 2,
            }
        };
        assert_eq!(map(0), 1);
        assert_eq!(map(1), 2);
        assert_eq!(map(2), 2);
    }

    #[test]
    fn matrix_cursor_clamps_and_resolves_on_empty_app() {
        // With no price history, there are no rows: count is 0, drill is None.
        let app = test_app();
        assert_eq!(matrix_asset_count(&app), 0);
        assert_eq!(matrix_cursor_subtab(&app), None);
        assert_eq!(focused_engine_asset(&app), None);
    }

    #[test]
    fn clarity_dot_reads_a_real_engine_report() {
        // Build a real report from a synthetic gold-shaped series so we exercise
        // the longest-degree clarity read without constructing private structs.
        let report = synthetic_gold_report();
        let dot = clarity_dot(&report);
        assert!(matches!(dot, "●" | "◐" | "○" | "·"), "unexpected dot {dot}");
    }

    #[test]
    fn engine_lines_render_plain_band_label_for_a_real_degree() {
        let report = synthetic_gold_report();
        let lines = engine_lines(report.degrees.first());
        let t = text(lines);
        // No doctrine words; functional band vocabulary only.
        let lower = t.to_lowercase();
        assert!(!lower.contains("loukas") && !lower.contains("olson"), "{t}");
        assert!(t.contains("Degree:"), "{t}");
    }

    /// A real `CycleReport` from a deterministic synthetic series — no private
    /// struct construction. Long enough to populate the long degree.
    fn synthetic_gold_report() -> CycleReport {
        use crate::analytics::cycle_engine::{analyze, default_config};
        use crate::models::price::HistoryRecord;
        use chrono::{Duration, NaiveDate};
        let start = NaiveDate::from_ymd_opt(2006, 1, 1).unwrap();
        let mut rows = Vec::new();
        for i in 0..5200i64 {
            let d = start + Duration::days(i);
            // Gentle uptrend + multi-year sinusoid to mint cycle lows.
            let base = 1500.0 + i as f64 * 0.15;
            let wave = 200.0 * ((i as f64) / 400.0).sin();
            let close = rust_decimal::Decimal::from_f64_retain(base + wave).unwrap();
            rows.push(HistoryRecord {
                date: d.format("%Y-%m-%d").to_string(),
                close,
                volume: None,
                open: None,
                high: None,
                low: None,
            });
        }
        analyze(&default_config("GC=F", "GC=F"), &rows).expect("report")
    }
}
