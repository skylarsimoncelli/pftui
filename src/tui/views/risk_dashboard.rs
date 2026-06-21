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

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let outer = Block::default()
        .title("Risk Dashboard")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_subtle))
        .style(Style::default().bg(app.theme.surface_0));
    frame.render_widget(outer, area);
    let inner = area.inner(Margin { horizontal: 1, vertical: 1 });
    if inner.height < 6 {
        return;
    }

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
}
