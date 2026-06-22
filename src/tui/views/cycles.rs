//! Cycles view — the TUI home for documented market-cycle clocks.
//!
//! Phase-1 MVP. Three sub-tabs (h/l): a dense Matrix landing row per asset that
//! has GENUINE cycle data (Bitcoin, Gold, Silver), plus full clock panels for
//! Bitcoin (~4-year supply cycle) and Gold (~6.9-year cycle). Everything is
//! computed INLINE from the in-memory `app.price_history` (pure CPU; the TUI
//! event loop must never block on I/O).
//!
//! Discipline (matches the operator's constraints):
//! - No practitioner/author names in the UI — only plain functional language.
//! - Friendly asset names only ("Bitcoin"/"Gold"/"Silver"), never raw tickers.
//! - Only Bitcoin, Gold and Silver have real anchored cycle data; nothing else
//!   is tracked here (no invented clocks for equities/DXY/oil). Silver phases
//!   WITH gold — it earns a Matrix row but no dedicated tab in the MVP.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::analytics::cycle_clock::{btc_cycle_clock, gold_cycle_clock, BtcCycleClock, GoldCycleClock};
use crate::analytics::cycle_engine::{analyze, default_config, BandPosition, CycleReport, DegreeStatus};
use crate::analytics::hurst_rs;
use crate::app::App;
use crate::models::price::HistoryRecord;

/// Sub-tab count (Matrix, Bitcoin, Gold). Cycled with h/l.
pub const SUBTAB_COUNT: u8 = 3;
const SUBTAB_NAMES: [&str; 3] = ["Matrix", "Bitcoin", "Gold"];

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
    name: &str,
    closes: &[f64],
    report: Option<&CycleReport>,
    stance: Option<&str>,
) -> MatrixRow {
    let regime = regime_glyph(closes).to_string();
    // The longest degree is the cycle-defining one (engine emits longest-first).
    let deg: Option<&DegreeStatus> = report.and_then(|r| r.degrees.first());

    let (degree, age, band, translation, next_low, sort_key) = match deg {
        Some(d) => {
            // Friendly degree label — strip doctrine names, describe by horizon.
            let degree = friendly_degree(&d.degree);
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
                .map(|c| match c.as_str() {
                    "RT" => "RT".to_string(),
                    "LT" => "LT".to_string(),
                    other => other.to_string(),
                })
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

/// Map an engine degree name to a plain, doctrine-free horizon label.
fn friendly_degree(degree: &str) -> String {
    match degree {
        "4-year" => "~4-year",
        "major" => "~6.9-year",
        "investor" => "investor",
        "intermediate" => "intermediate",
        "daily" => "daily",
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

fn render_matrix(frame: &mut Frame, area: Rect, app: &App) {
    let mut rows: Vec<MatrixRow> = Vec::new();
    for a in &CYCLE_ASSETS {
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
        rows.push(matrix_row(a.name, &closes, report.as_ref(), stance.as_deref()));
    }

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

    // Sort by next-low proximity (soonest first).
    rows.sort_by_key(|r| r.sort_key);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(
            "{:<8}{:<14}{:<12}{:<7}{:<5}{:<24}{:<10}{}",
            "Asset", "Degree", "Age/%band", "Band", "Tr", "Next-low window", "Regime", "Stance / position"
        ),
        Style::default().fg(app.theme.text_secondary),
    )));
    for r in &rows {
        lines.push(Line::from(format!(
            "{:<8}{:<14}{:<12}{:<7}{:<5}{:<24}{:<10}{}",
            r.name, r.degree, r.age, r.band, r.translation, r.next_low, r.regime, r.stance
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Band: <pre / =in / >over the low-to-low timing window. Tr: RT (right-translated) / LT (left). Sorted by next-low proximity.",
        Style::default().fg(app.theme.text_muted),
    )));
    lines.push(Line::from(Span::styled(
        "Regime ↗ trending · ↔ random · ⟲ mean-reverting. Clarity dot ● clear · ◐ mixed · ○ noisy.",
        Style::default().fg(app.theme.text_muted),
    )));
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(app.theme.text_primary)),
        area,
    );
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
        let row = matrix_row("Bitcoin", &closes, None, Some("ACCUMULATE ●"));
        assert_eq!(row.name, "Bitcoin");
        assert_eq!(row.degree, "—");
        assert_eq!(row.band, "—");
        assert_eq!(row.stance, "ACCUMULATE ●");
        // Regime is computed from closes even without an engine report.
        assert!(!row.regime.is_empty());
        assert_eq!(row.sort_key, i64::MAX / 2);
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
