//! Risk-Dashboard view — the TUI analogue of `pftui analytics risk-dashboard`.
//! For the focused asset it composes the native risk family — volatility &
//! drawdown, EVT tail risk, drawdown-path (CDaR/Ulcer/Omega), Hurst/DFA regime,
//! and Triple-Penance survival/ruin — into a 2×2 panel grid. Everything is
//! computed INLINE from the in-memory `app.price_history` (pure CPU; the TUI
//! event loop must never block on I/O), so it reflects whatever asset the
//! operator has selected.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::analytics::{drawdown_metrics, evt, hurst_rs, risk, survival};
use crate::app::App;

use super::analytics::focus_symbol_closes;

/// Number of sub-tabs (Risk grid, Basket, Cycle clock, Diversification). h/l.
pub const SUBTAB_COUNT: u8 = 4;
const SUBTAB_NAMES: [&str; 4] = [
    "Risk (asset)",
    "Basket (allocation)",
    "Cycle (BTC/gold)",
    "Diversification",
];

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let active = (app.risk_subtab % SUBTAB_COUNT) as usize;
    let outer = Block::default()
        .title(format!("Risk Dashboard — {}", SUBTAB_NAMES[active]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_subtle))
        .style(Style::default().bg(app.theme.surface_0));
    frame.render_widget(outer, area);
    let inner = area.inner(Margin { horizontal: 1, vertical: 1 });
    if inner.height < 6 {
        return;
    }
    // Sub-tab strip (h/l to switch) + content area.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(inner);
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
    frame.render_widget(Paragraph::new(Line::from(spans)), rows[0]);

    match active {
        1 => render_basket(frame, rows[1], app),
        2 => render_cycle(frame, rows[1], app),
        3 => render_correlation(frame, rows[1], app),
        _ => render_risk_grid(frame, rows[1], app),
    }
}

/// Diversification sub-tab: pairwise correlation + co-crash tail-dependence
/// (empirical lower-tail λ_L) across the held basket — "does my book actually
/// diversify, including in a crash?". Computed inline over the most-recent ~1y
/// of common history (bounds the O(n²) Kendall-τ so it's cheap per frame).
fn render_correlation(frame: &mut Frame, area: Rect, app: &App) {
    let symbols = priceable_held_symbols(app, 8);
    if symbols.len() < 2 {
        frame.render_widget(
            Paragraph::new("Need ≥2 held assets with cached history to assess pairwise diversification.")
                .style(Style::default().fg(app.theme.text_muted)),
            area,
        );
        return;
    }
    let series = match aligned_returns(app, &symbols) {
        Some(s) => s,
        None => {
            frame.render_widget(
                Paragraph::new("Held assets don't share ≥21 common trading days yet.")
                    .style(Style::default().fg(app.theme.text_muted)),
                area,
            );
            return;
        }
    };
    let pairs = pair_diversification(&series, &symbols, 252);
    if pairs.is_empty() {
        frame.render_widget(
            Paragraph::new("Not enough common history (need ≥100 shared days) for any pair.")
                .style(Style::default().fg(app.theme.text_muted)),
            area,
        );
        return;
    }
    let mut lines = vec![
        Line::from(Span::styled(
            "Pairwise correlation & co-crash λ_L over the last ~1y (most co-crashing first)",
            Style::default().fg(app.theme.text_secondary),
        )),
        Line::from(""),
    ];
    for (a, b, pearson, lambda) in &pairs {
        let verdict = if *lambda >= 0.40 {
            "STRONG — diversification fails in a crash"
        } else if *lambda >= 0.20 {
            "MODERATE co-crash"
        } else {
            "WEAK — diversification holds"
        };
        lines.push(Line::from(format!(
            "{a}↔{b}:  corr {pearson:+.2}  ·  co-crash λ_L {lambda:.2}  ({verdict})"
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "λ_L = chance both crash together in the worst 5% of days. Low = the pair truly diversifies.",
        Style::default().fg(app.theme.text_muted),
    )));
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(app.theme.text_primary)),
        area,
    );
}

/// Held symbols (input order) with ≥21 positive in-memory closes, capped at
/// `max` for a bounded display/compute. Pure read of `app`.
fn priceable_held_symbols(app: &App, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for p in &app.positions {
        if out.len() >= max {
            break;
        }
        if out.iter().any(|s| s == &p.symbol) {
            continue;
        }
        let ok = app
            .price_history
            .get(&p.symbol)
            .map(|h| h.iter().filter(|r| r.close > rust_decimal::Decimal::ZERO).count() >= 21)
            .unwrap_or(false);
        if ok {
            out.push(p.symbol.clone());
        }
    }
    out
}

/// Pure pairwise (Pearson, co-crash λ_L) for every asset pair, each computed on
/// the most-recent `window` returns (bounds the O(n²) Kendall-τ). Sorted by λ_L
/// descending (most-co-crashing first). Pairs with <100 windowed points are
/// dropped (`tail_dependence` needs ≥100). Testable without an `App`.
fn pair_diversification(
    series: &[Vec<f64>],
    symbols: &[String],
    window: usize,
) -> Vec<(String, String, f64, f64)> {
    let mut out = Vec::new();
    for i in 0..symbols.len() {
        let wi = &series[i][series[i].len().saturating_sub(window)..];
        for j in (i + 1)..symbols.len() {
            let wj = &series[j][series[j].len().saturating_sub(window)..];
            if let Some(td) = crate::analytics::copula::tail_dependence(wi, wj, 0.05) {
                out.push((symbols[i].clone(), symbols[j].clone(), td.pearson, td.emp_lower_tail_dep));
            }
        }
    }
    out.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Cycle sub-tab: the asset's market-cycle clock (BTC 4-year halving cycle /
/// gold ~6.9-year cycle) — accumulation/distribution timing for the focused
/// asset. Defined only for BTC and gold (the cycle-accumulation pair). Computed
/// inline from the in-memory `price_history` (pure; no I/O).
fn render_cycle(frame: &mut Frame, area: Rect, app: &App) {
    use crate::analytics::cycle_clock::{btc_cycle_clock, gold_cycle_clock};
    use crate::analytics::strategy::resolver::resolve_alias;

    let sym = match focus_symbol_closes(app) {
        Some((s, _)) => s,
        None => {
            frame.render_widget(
                Paragraph::new("Select BTC or gold to see its market-cycle clock.")
                    .style(Style::default().fg(app.theme.text_muted)),
                area,
            );
            return;
        }
    };
    let resolved = resolve_alias(&sym);
    let hist = app.price_history.get(&sym);
    let mut lines: Vec<Line<'static>> = Vec::new();
    match (resolved.as_str(), hist) {
        ("BTC-USD", Some(h)) if h.len() >= 100 => match btc_cycle_clock(&resolved, h) {
            Some(c) => {
                lines.push(Line::from(Span::styled(
                    format!("BTC cycle · {} · {:.0}", c.as_of, c.last_close),
                    Style::default().fg(app.theme.text_secondary),
                )));
                lines.push(Line::from(format!(
                    "Stance: {} (score {:+})",
                    c.accumulation.stance, c.accumulation.score
                )));
                lines.push(Line::from(format!(
                    "Halving +{}w · Olson bottom in {}d",
                    c.weeks_since_halving, c.olson_days_remaining
                )));
                if let Some(p) = c.pct_vs_200wma {
                    lines.push(Line::from(format!("vs 200w-MA: {p:+.0}%")));
                }
                lines.push(Line::from(""));
                for fac in c.accumulation.factors.iter().take(4) {
                    lines.push(Line::from(format!("· {fac}")));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    c.verdict.clone(),
                    Style::default().fg(app.theme.text_accent),
                )));
            }
            None => lines.push(Line::from("BTC cycle clock unavailable (insufficient history).")),
        },
        ("GC=F", Some(h)) if h.len() >= 100 => match gold_cycle_clock(&resolved, h) {
            Some(c) => {
                lines.push(Line::from(Span::styled(
                    format!("Gold cycle · {} · {:.0}", c.as_of, c.last_close),
                    Style::default().fg(app.theme.text_secondary),
                )));
                if let Some(pos) = c.cycle_position_pct {
                    lines.push(Line::from(format!("Cycle position: {pos:.0}% through")));
                }
                if let Some(y) = c.years_since_cycle_low {
                    lines.push(Line::from(format!("Years since cycle low: {y:.1}")));
                }
                if let Some(half) = c.past_half_cycle {
                    lines.push(Line::from(format!(
                        "Past half-cycle: {}",
                        if half { "yes (2nd half)" } else { "no (1st half)" }
                    )));
                }
                if let Some(ext) = c.extension_pct_vs_200dma {
                    lines.push(Line::from(format!("vs 200d-MA: {ext:+.0}%")));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    c.verdict.clone(),
                    Style::default().fg(app.theme.text_accent),
                )));
            }
            None => lines.push(Line::from("Gold cycle clock unavailable (insufficient history).")),
        },
        _ => {
            lines.push(Line::from(Span::styled(
                "The market-cycle clock is defined for BTC and gold — the cycle-accumulation pair.",
                Style::default().fg(app.theme.text_muted),
            )));
            lines.push(Line::from(Span::styled(
                "Select BTC or gold (Positions/Markets) to see accumulation/distribution timing.",
                Style::default().fg(app.theme.text_muted),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(app.theme.text_primary)),
        area,
    );
}

fn render_risk_grid(frame: &mut Frame, inner: Rect, app: &App) {
    let (symbol, closes) = match focus_symbol_closes(app) {
        Some(x) => x,
        None => {
            let hint = Paragraph::new(
                "Select an asset with price history (Positions/Markets) to see its risk dashboard.\n\nThe selected asset's volatility, tail risk, drawdown-path, regime and survival appear here.",
            )
            .style(Style::default().fg(app.theme.text_muted));
            frame.render_widget(hint, inner);
            return;
        }
    };

    // --- compute the risk family once, inline, from in-memory closes ---
    let returns: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let log_rets: Vec<f64> = closes
        .windows(2)
        .filter(|w| w[0] > 0.0 && w[1] > 0.0)
        .map(|w| (w[1] / w[0]).ln())
        .collect();
    let closes_dec: Vec<rust_decimal::Decimal> = closes
        .iter()
        .filter_map(|c| rust_decimal::Decimal::from_f64_retain(*c))
        .collect();

    let vol = risk::annualized_volatility_pct(&returns).and_then(rd_to_f64);
    let max_dd = risk::max_drawdown_pct(&closes_dec).and_then(rd_to_f64);
    let price = closes.last().copied().unwrap_or(0.0);
    let ath = closes.iter().cloned().fold(f64::MIN, f64::max);
    let dd_from_ath = if ath > 0.0 { (price / ath - 1.0) * 100.0 } else { 0.0 };
    let e = evt::fit_evt_tail_risk(&returns, 0.95);
    let dd = drawdown_metrics::compute(&closes, None, 0.0);
    let cdar95 = dd.as_ref().map(|d| d.cdar_95);
    let h = hurst_rs::hurst(&log_rets);
    let s = survival::compute(&log_rets, cdar95, 25.0, 0.95);

    // 2×2 grid.
    let header = Paragraph::new(format!("{symbol} · {} closes · price {price:.2}", closes.len()))
        .style(Style::default().fg(app.theme.text_secondary));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(inner);
    frame.render_widget(header, rows[0]);
    let grid = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(grid[0]);
    let bot = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(grid[1]);

    panel(frame, top[0], app, "Volatility & Tail", vol_tail_lines(vol, max_dd, dd_from_ath, &e));
    panel(frame, top[1], app, "Drawdown Path", drawdown_lines(dd.as_ref()));
    panel(frame, bot[0], app, "Regime", regime_lines(h.as_ref()));
    panel(frame, bot[1], app, "Survival", survival_lines(s.as_ref()));
}

/// Basket sub-tab: current portfolio weights vs the risk-equalized (risk-parity
/// & downside-risk-parity) weights across the held basket — all computed inline
/// from the in-memory `price_history` (no blocking I/O).
fn render_basket(frame: &mut Frame, area: Rect, app: &App) {
    use rust_decimal::prelude::ToPrimitive;
    // Held assets with a current value → current weights.
    let held: Vec<(String, f64)> = app
        .positions
        .iter()
        .filter_map(|p| p.current_value.and_then(|v| v.to_f64()).map(|v| (p.symbol.clone(), v)))
        .filter(|(_, v)| *v > 0.0)
        .collect();
    let total: f64 = held.iter().map(|(_, v)| v).sum();
    // Keep only held assets that have enough in-memory history.
    let symbols: Vec<String> = held
        .iter()
        .filter(|(s, _)| {
            app.price_history
                .get(s)
                .map(|h| h.iter().filter(|r| r.close > rust_decimal::Decimal::ZERO).count() >= 21)
                .unwrap_or(false)
        })
        .map(|(s, _)| s.clone())
        .collect();
    if symbols.len() < 2 || total <= 0.0 {
        let hint = Paragraph::new(
            "Need at least 2 held assets with cached price history for a risk-parity allocation check.\n\nShows your current weight vs the equal-risk (risk-parity / downside-RP) weights.",
        )
        .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(hint, area);
        return;
    }
    let series = match aligned_returns(app, &symbols) {
        Some(s) => s,
        None => {
            let hint = Paragraph::new("Held assets don't share ≥21 common trading days yet.")
                .style(Style::default().fg(app.theme.text_muted));
            frame.render_widget(hint, area);
            return;
        }
    };
    use crate::analytics::basket::{allocate, Method};
    let rp = allocate(&symbols, &series, Method::RiskParity);
    let drp = allocate(&symbols, &series, Method::DownsideRiskParity);
    let (rp, drp) = match (rp, drp) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            let hint = Paragraph::new("Could not compute allocation (degenerate covariance).")
                .style(Style::default().fg(app.theme.text_muted));
            frame.render_widget(hint, area);
            return;
        }
    };
    let cur_pct = |sym: &str| {
        held.iter().find(|(s, _)| s == sym).map(|(_, v)| v / total * 100.0).unwrap_or(0.0)
    };
    let rp_pct = |sym: &str| rp.weights.iter().find(|w| w.symbol == sym).map(|w| w.weight * 100.0).unwrap_or(0.0);
    let drp_pct = |sym: &str| drp.weights.iter().find(|w| w.symbol == sym).map(|w| w.weight * 100.0).unwrap_or(0.0);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("{:<10}{:>8}{:>10}{:>11}{:>9}", "Asset", "Current", "Risk-par", "Downside", "Gap"),
            Style::default().fg(app.theme.text_secondary),
        )),
    ];
    // Sort most-overweight-risk first.
    let mut order: Vec<&String> = symbols.iter().collect();
    order.sort_by(|a, b| {
        (cur_pct(b) - rp_pct(b))
            .partial_cmp(&(cur_pct(a) - rp_pct(a)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for sym in order {
        let (c, r, d) = (cur_pct(sym), rp_pct(sym), drp_pct(sym));
        lines.push(Line::from(format!(
            "{:<10}{:>7.0}%{:>9.0}%{:>10.0}%{:>+8.0}",
            sym, c, r, d, c - r
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Gap = current − risk-parity (pp). Positive = you carry more of the portfolio's RISK than equal-risk.",
        Style::default().fg(app.theme.text_muted),
    )));
    let p = Paragraph::new(lines).style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

/// Align held symbols' in-memory closes on their COMMON dates → per-asset simple
/// returns (same order as `symbols`). Pure read of `price_history`; `None` if
/// any symbol is missing or fewer than 21 common dates exist.
fn aligned_returns(app: &App, symbols: &[String]) -> Option<Vec<Vec<f64>>> {
    use rust_decimal::prelude::ToPrimitive;
    use std::collections::BTreeMap;
    let maps: Vec<BTreeMap<String, f64>> = symbols
        .iter()
        .map(|s| {
            app.price_history
                .get(s)
                .map(|recs| {
                    recs.iter()
                        .filter_map(|r| r.close.to_f64().map(|c| (r.date.clone(), c)))
                        .filter(|(_, c)| *c > 0.0)
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    align_common(&maps)
}

/// Pure core of [`aligned_returns`]: intersect the per-asset date→close maps on
/// their common dates and difference into simple returns. `None` if any map is
/// empty or fewer than 21 common dates. Testable without an `App`.
fn align_common(maps: &[std::collections::BTreeMap<String, f64>]) -> Option<Vec<Vec<f64>>> {
    if maps.len() < 2 || maps.iter().any(|m| m.is_empty()) {
        return None;
    }
    let mut common: Vec<&String> = maps[0].keys().collect();
    common.retain(|d| maps[1..].iter().all(|m| m.contains_key(*d)));
    common.sort();
    if common.len() < 21 {
        return None;
    }
    Some(
        maps.iter()
            .map(|m| common.windows(2).map(|w| m[w[1]] / m[w[0]] - 1.0).collect())
            .collect(),
    )
}

fn rd_to_f64(d: rust_decimal::Decimal) -> Option<f64> {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64()
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

fn opt_pct(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}%")).unwrap_or_else(|| "—".into())
}

fn vol_tail_lines(
    vol: Option<f64>,
    max_dd: Option<f64>,
    dd_from_ath: f64,
    e: &Option<evt::EvtTailRisk>,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("Vol (ann):  {}", opt_pct(vol))),
        Line::from(format!("Max DD:     {}", opt_pct(max_dd))),
        Line::from(format!("From ATH:   {dd_from_ath:+.1}%")),
    ];
    match e {
        Some(e) => {
            lines.push(Line::from(format!("EVT ξ:      {:+.2} ({})", e.xi, e.tail_class)));
            lines.push(Line::from(format!(
                "VaR 99/99.9: {:.1}% / {:.1}%",
                e.var_99_pct, e.var_999_pct
            )));
            lines.push(Line::from(format!("ES99:       {:.1}%", e.es_99_pct)));
        }
        None => lines.push(Line::from("EVT: insufficient data")),
    }
    lines
}

fn drawdown_lines(dd: Option<&drawdown_metrics::DrawdownMetrics>) -> Vec<Line<'static>> {
    match dd {
        Some(d) => vec![
            Line::from(format!("CDaR-95:  {:.1}%", d.cdar_95 * 100.0)),
            Line::from(format!("CDaR-90:  {:.1}%", d.cdar_90 * 100.0)),
            Line::from(format!("Ulcer:    {:.1}%", d.ulcer_index_pct)),
            Line::from(format!(
                "Omega(τ0):{}",
                d.omega_ratio.map(|v| format!(" {v:.2}")).unwrap_or_else(|| " —".into())
            )),
            Line::from("(tail of the drawdown distribution)"),
        ],
        None => vec![Line::from("Drawdown-path: insufficient data")],
    }
}

fn regime_lines(h: Option<&hurst_rs::HurstResult>) -> Vec<Line<'static>> {
    match h {
        Some(h) => vec![
            Line::from(format!("Hurst H:  {:.2} ({})", h.h, h.regime)),
            Line::from(format!(
                "DFA α:    {}",
                h.dfa_alpha.map(|a| format!("{a:.2}")).unwrap_or_else(|| "—".into())
            )),
            Line::from(format!("Agree:    {}", h.agreement)),
            Line::from("(>0.5 trending · <0.5 mean-revert)"),
        ],
        None => vec![Line::from("Regime: insufficient data")],
    }
}

fn survival_lines(s: Option<&survival::Survival>) -> Vec<Line<'static>> {
    match s {
        Some(s) if s.reliable => vec![
            Line::from(format!("Ruin @25%: {:.0}%", s.ruin_prob * 100.0)),
            Line::from(format!(
                "Max DD@95: {}",
                s.max_dd_iid.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into())
            )),
            Line::from(format!(
                "Underwater:{}",
                s.max_tuw_iid_days.map(|d| format!(" {:.1}y", d / 365.25)).unwrap_or_else(|| " —".into())
            )),
            Line::from(format!(
                "Recovery:  {}",
                s.recovery_required_at_cdar95
                    .map(|r| format!("+{:.0}% to erase CDaR", r * 100.0))
                    .unwrap_or_else(|| "—".into())
            )),
        ],
        Some(s) => vec![
            Line::from(format!("Ruin @25%: {:.0}%", s.ruin_prob * 100.0)),
            Line::from("No positive drift —"),
            Line::from("recovery unbounded in"),
            Line::from("expectation (cycle low)."),
        ],
        None => vec![Line::from("Survival: insufficient data")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The panel line-builders are pure given the analytics outputs; verify they
    // render the expected labels for a synthetic positive-drift series without
    // constructing a full App.
    #[test]
    fn panels_render_expected_labels_for_a_drifting_series() {
        let closes: Vec<f64> = (0..400)
            .map(|i| 100.0 * (1.0 + 0.0008 * i as f64) + 5.0 * (i as f64 / 9.0).sin())
            .collect();
        let returns: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
        let log_rets: Vec<f64> = closes.windows(2).map(|w| (w[1] / w[0]).ln()).collect();

        let e = evt::fit_evt_tail_risk(&returns, 0.95);
        let dd = drawdown_metrics::compute(&closes, None, 0.0);
        let cdar95 = dd.as_ref().map(|d| d.cdar_95);
        let h = hurst_rs::hurst(&log_rets);
        let s = survival::compute(&log_rets, cdar95, 25.0, 0.95);

        let text = |lines: Vec<Line>| {
            lines
                .iter()
                .flat_map(|l| l.spans.iter().map(|sp| sp.content.to_string()))
                .collect::<Vec<_>>()
                .join(" | ")
        };

        assert!(text(vol_tail_lines(Some(20.0), Some(-30.0), -5.0, &e)).contains("Vol (ann)"));
        assert!(text(drawdown_lines(dd.as_ref())).contains("CDaR-95"));
        assert!(text(regime_lines(h.as_ref())).contains("Hurst"));
        let surv = text(survival_lines(s.as_ref()));
        assert!(surv.contains("Ruin"), "survival panel should show ruin: {surv}");
    }

    #[test]
    fn align_common_intersects_dates_and_differences_returns() {
        use std::collections::BTreeMap;
        // Two assets, 30 shared dates + a few non-shared → 30 common, 29 returns.
        let mk = |offset: f64, extra: &[&str]| -> BTreeMap<String, f64> {
            let mut m = BTreeMap::new();
            for i in 0..30 {
                m.insert(format!("2026-01-{:02}", i + 1), 100.0 + offset + i as f64);
            }
            for d in extra {
                m.insert(d.to_string(), 999.0);
            }
            m
        };
        let a = mk(0.0, &["2025-12-31"]); // a-only date
        let b = mk(50.0, &["2026-03-01"]); // b-only date
        let series = align_common(&[a, b]).expect("≥21 common dates");
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].len(), 29, "30 common dates → 29 returns");
        assert_eq!(series[1].len(), 29);
        // Non-overlapping baskets / single asset → None.
        let lonely = {
            let mut m = BTreeMap::new();
            m.insert("2026-01-01".into(), 1.0);
            m
        };
        assert!(align_common(std::slice::from_ref(&lonely)).is_none());
    }

    #[test]
    fn pair_diversification_windows_sorts_and_drops_thin_pairs() {
        // 3 assets, 150 returns each. A & B nearly identical (high λ_L), C is an
        // independent-ish sawtooth. Expect 3 pairs, sorted by λ_L desc; the most
        // co-crashing pair (A↔B) first. Window=120 bounds the input (≥100 ok).
        let n = 150;
        let a: Vec<f64> = (0..n).map(|t| ((t as f64) * 0.5).sin() * 0.03).collect();
        let b: Vec<f64> = a.iter().map(|x| x + 0.0001).collect(); // ~identical → co-crash
        let c: Vec<f64> = (0..n).map(|t| if t % 2 == 0 { 0.01 } else { -0.012 }).collect();
        let syms = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let pairs = pair_diversification(&[a, b, c], &syms, 120);
        assert_eq!(pairs.len(), 3, "3 assets → 3 pairs");
        // Sorted by λ_L desc → first pair is the most co-crashing.
        assert!(pairs[0].3 >= pairs[1].3 && pairs[1].3 >= pairs[2].3, "λ_L not sorted desc");
        assert!(pairs.iter().any(|(x, y, _, _)| (x == "A" && y == "B") || (x == "B" && y == "A")));
        // A pair with <100 windowed points is dropped.
        let short: Vec<Vec<f64>> = vec![vec![0.01; 50], vec![0.02; 50]];
        assert!(pair_diversification(&short, &["X".into(), "Y".into()], 252).is_empty());
    }
}
