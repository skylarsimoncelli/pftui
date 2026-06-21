use std::collections::HashMap;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::analytics::risk;
use crate::analytics::scenarios::{apply_preset, ScenarioPreset};
use crate::app::App;
use crate::config::PortfolioMode;
use crate::models::asset::AssetCategory;

const SCENARIOS: &[(ScenarioPreset, &str)] = &[
    (ScenarioPreset::Oil100, "Oil $100"),
    (ScenarioPreset::Btc40k, "BTC $40k"),
    (ScenarioPreset::Gold6000, "Gold $6000"),
    (ScenarioPreset::Gfc2008, "2008 GFC"),
    (ScenarioPreset::OilCrisis1973, "1973 Oil Crisis"),
];

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let main = Block::default()
        .title("Analytics")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_subtle))
        .style(Style::default().bg(app.theme.surface_0));
    frame.render_widget(main, area);
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.height < 8 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(6),
        ])
        .split(inner);

    let portfolio_values: Vec<Decimal> = app
        .portfolio_value_history
        .iter()
        .map(|(_, v)| *v)
        .collect();
    let position_values: Vec<Decimal> = app
        .positions
        .iter()
        .filter_map(|p| p.current_value)
        .collect();
    let risk_metrics = risk::compute_risk_metrics(&portfolio_values, &position_values, None);
    render_risk_panel(frame, rows[0], app, &risk_metrics);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[1]);
    render_concentration_panel(frame, mid[0], app, risk_metrics.herfindahl_index);
    render_scenarios_panel(frame, mid[1], app);

    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[2]);
    render_projection_panel(frame, lower[0], app);
    render_regime_monitor(frame, lower[1], app);

    render_impact_panel(frame, rows[3], app);
}

fn render_risk_panel(frame: &mut Frame, area: Rect, app: &App, metrics: &risk::RiskMetrics) {
    // Survival headline for the focused asset (the native survival/CDaR
    // analytics, surfaced in the TUI). Computed inline from the in-memory
    // `price_history` — pure CPU, never blocking I/O (TUI event-loop rule).
    let survival_line = match focus_symbol_closes(app) {
        Some((sym, closes)) => match survival_summary_text(&closes, &sym) {
            Some(text) => Line::from(Span::styled(text, Style::default().fg(app.theme.text_secondary))),
            // We're in the ≥31-closes arm, so a None here means a degenerate
            // (flat / zero-variance) return series, NOT insufficient history.
            None => Line::from(Span::styled(
                format!("Survival ({sym}): not computable (flat/degenerate return series)."),
                Style::default().fg(app.theme.text_muted),
            )),
        },
        None => Line::from(Span::styled(
            "Survival: select an asset with price history for a ruin/time-underwater read.",
            Style::default().fg(app.theme.text_muted),
        )),
    };
    let lines = vec![
        Line::from(format!(
            "Vol (ann): {}    Sharpe: {}",
            fmt_pct_opt(metrics.annualized_volatility_pct),
            fmt_num_opt(metrics.sharpe_ratio)
        )),
        Line::from(format!(
            "Max Drawdown: {}    Hist VaR 95: {}",
            fmt_pct_opt(metrics.max_drawdown_pct),
            fmt_pct_opt(metrics.historical_var_95_pct)
        )),
        Line::from(format!(
            "Concentration (HHI): {}",
            fmt_num_opt(metrics.herfindahl_index)
        )),
        survival_line,
    ];
    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Risk Panel")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn render_concentration_panel(frame: &mut Frame, area: Rect, app: &App, hhi: Option<Decimal>) {
    let mut weighted: Vec<(String, Decimal)> = app
        .positions
        .iter()
        .filter_map(|p| p.current_value.map(|v| (p.symbol.clone(), v)))
        .collect();
    weighted.sort_by_key(|b| std::cmp::Reverse(b.1));
    let total: Decimal = weighted.iter().map(|(_, v)| *v).sum();

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (sym, value) in weighted.iter().take(4) {
        let pct = if total > dec!(0) {
            (*value / total) * dec!(100)
        } else {
            dec!(0)
        };
        let bars = ((pct / dec!(5)).floor().to_i32().unwrap_or(0)).clamp(0, 20) as usize;
        lines.push(Line::from(format!(
            "{:<8} {:>6.1}% {}",
            sym,
            pct,
            "█".repeat(bars)
        )));
    }
    if let Some(h) = hhi {
        let flag = if h >= dec!(0.25) {
            "HIGH"
        } else if h >= dec!(0.15) {
            "MODERATE"
        } else {
            "LOW"
        };
        lines.push(Line::from(""));
        lines.push(Line::from(format!("HHI Risk Flag: {}", flag)));
    }

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Concentration")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn render_scenarios_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    app.page_table_area = Some(area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (idx, (_, label)) in SCENARIOS.iter().enumerate() {
        let marker = if idx == app.analytics_selected_index {
            ">"
        } else {
            " "
        };
        lines.push(Line::from(format!("{marker} {}", label)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Shock Scale: {}%",
        app.analytics_shock_scale_pct
    )));
    lines.push(Line::from(Span::styled(
        "Use j/k to select, +/- to tweak, 0 reset.",
        Style::default().fg(app.theme.text_muted),
    )));

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Scenarios")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn render_projection_panel(frame: &mut Frame, area: Rect, app: &App) {
    let (preset, label) = SCENARIOS[app.analytics_selected_index.min(SCENARIOS.len() - 1)];
    let overrides = scaled_overrides(preset, &app.prices, app.analytics_shock_scale_pct);

    let current: Decimal = app.positions.iter().filter_map(|p| p.current_value).sum();
    let projected = projected_value(app, &overrides);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(format!("Selected Scenario: {}", label)));
    lines.push(Line::from(format!(
        "Current Value: {}{:.2}",
        crate::config::currency_symbol(&app.base_currency),
        current
    )));

    match projected {
        Some(value) => {
            let delta = value - current;
            let pct = if current > dec!(0) {
                (delta / current) * dec!(100)
            } else {
                dec!(0)
            };
            lines.push(Line::from(format!(
                "Projected Value: {}{:.2} ({:+.2}%)",
                crate::config::currency_symbol(&app.base_currency),
                value,
                pct
            )));
            lines.push(Line::from(format!(
                "Projected Delta: {}{:+.2}",
                crate::config::currency_symbol(&app.base_currency),
                delta
            )));
        }
        None => {
            lines.push(Line::from("Projected Value: N/A"));
            if app.portfolio_mode == PortfolioMode::Percentage {
                lines.push(Line::from(
                    "Percentage mode uses target weights; no quantity-based projection.",
                ));
            }
        }
    }

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Scenario Projection")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn render_regime_monitor(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(format!(
            "Regime: {} ({:+}/{})",
            app.regime_score.label(),
            app.regime_score.total,
            app.regime_score.active_count
        )),
        Line::from(format!(
            "Crypto F&G: {}",
            app.crypto_fng
                .as_ref()
                .map(|(value, label)| format!("{value} {label}"))
                .unwrap_or_else(|| "N/A".to_string())
        )),
        Line::from(format!(
            "Traditional F&G: {}",
            app.traditional_fng
                .as_ref()
                .map(|(value, label)| format!("{value} {label}"))
                .unwrap_or_else(|| "N/A".to_string())
        )),
        Line::from(""),
    ];

    for signal in app.regime_score.signals.iter().take(4) {
        lines.push(Line::from(format!(
            "{} {}",
            if signal.score > 0 {
                "↑"
            } else if signal.score < 0 {
                "↓"
            } else {
                "·"
            },
            signal.label
        )));
    }

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Regime Monitor")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn render_impact_panel(frame: &mut Frame, area: Rect, app: &App) {
    let (preset, label) = SCENARIOS[app.analytics_selected_index.min(SCENARIOS.len() - 1)];
    let overrides = scaled_overrides(preset, &app.prices, app.analytics_shock_scale_pct);
    let mut impacts: Vec<(String, Decimal)> = app
        .positions
        .iter()
        .filter_map(|pos| {
            let current = pos.current_price?;
            let target = overrides.get(&pos.symbol).copied().unwrap_or(current);
            let delta_pct = if current > dec!(0) {
                (target - current) / current * dec!(100)
            } else {
                dec!(0)
            };
            Some((pos.symbol.clone(), delta_pct))
        })
        .collect();
    impacts.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()).then_with(|| a.0.cmp(&b.0)));

    let mut lines = vec![
        Line::from(format!("Scenario impact ranking for {}", label)),
        Line::from(""),
    ];
    for (symbol, delta_pct) in impacts.into_iter().take(6) {
        lines.push(Line::from(format!("{:<8} {:+.2}%", symbol, delta_pct)));
    }
    if !app.calendar_events.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Next calendar event",
            Style::default().fg(app.theme.text_secondary).bold(),
        )));
        if let Some(event) = app.calendar_events.first() {
            lines.push(Line::from(format!("{} {}", event.date, event.name)));
        }
    }

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Impact Map")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border_subtle)),
        )
        .style(Style::default().fg(app.theme.text_primary));
    frame.render_widget(p, area);
}

fn scaled_overrides(
    preset: ScenarioPreset,
    prices: &HashMap<String, Decimal>,
    scale_pct: i32,
) -> HashMap<String, Decimal> {
    let raw = apply_preset(preset, prices);
    if scale_pct == 100 {
        return raw;
    }

    let scale = Decimal::from(scale_pct) / dec!(100);
    let mut out = HashMap::new();
    for (sym, target) in raw {
        let base = prices.get(&sym).copied().unwrap_or(target);
        let scaled = base + (target - base) * scale;
        out.insert(sym, scaled.max(Decimal::ZERO));
    }
    out
}

fn projected_value(app: &App, overrides: &HashMap<String, Decimal>) -> Option<Decimal> {
    if app.portfolio_mode == PortfolioMode::Percentage {
        return None;
    }

    let mut total = dec!(0);
    for pos in &app.positions {
        let px = if pos.category == AssetCategory::Cash {
            dec!(1)
        } else {
            overrides.get(&pos.symbol).copied().or(pos.current_price)?
        };
        total += px * pos.quantity;
    }
    Some(total)
}

fn fmt_pct_opt(v: Option<Decimal>) -> String {
    match v {
        Some(x) => format!("{:.2}%", x),
        None => "N/A".to_string(),
    }
}

fn fmt_num_opt(v: Option<Decimal>) -> String {
    match v {
        Some(x) => format!("{:.3}", x),
        None => "N/A".to_string(),
    }
}

/// The asset to focus risk analytics on: the selected symbol if it has enough
/// in-memory history, else the first held position that does. Returns
/// `(symbol, positive closes)`. Pure read of `app` state — no I/O. Shared with
/// the Risk-Dashboard view.
pub(crate) fn focus_symbol_closes(app: &App) -> Option<(String, Vec<f64>)> {
    let closes_for = |sym: &str| -> Option<Vec<f64>> {
        let v: Vec<f64> = app
            .price_history
            .get(sym)?
            .iter()
            .filter_map(|r| r.close.to_f64())
            .filter(|c| *c > 0.0)
            .collect();
        (v.len() >= 31).then_some(v)
    };
    if let Some(sel) = &app.selected_symbol {
        if let Some(v) = closes_for(sel) {
            return Some((sel.clone(), v));
        }
    }
    for p in &app.positions {
        if let Some(v) = closes_for(&p.symbol) {
            return Some((p.symbol.clone(), v));
        }
    }
    None
}

/// Pure one-line survival headline for a symbol's close series (ruin vs a 25%
/// budget, arithmetic max-DD, CDaR-95, total time-under-water), or `None` with
/// <31 closes. Testable without the TUI/App.
fn survival_summary_text(closes: &[f64], symbol: &str) -> Option<String> {
    if closes.len() < 31 {
        return None;
    }
    let log_rets: Vec<f64> = closes
        .windows(2)
        .filter(|w| w[0] > 0.0 && w[1] > 0.0)
        .map(|w| (w[1] / w[0]).ln())
        .collect();
    let cdar = crate::analytics::drawdown_metrics::compute(closes, None, 0.0).map(|d| d.cdar_95);
    let s = crate::analytics::survival::compute(&log_rets, cdar, 25.0, 0.95)?;
    let cdar_s = cdar.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into());
    Some(if s.reliable {
        format!(
            "Survival ({symbol}): ruin {:.0}% · max-DD {} · CDaR95 {cdar_s} · underwater {}",
            s.ruin_prob * 100.0,
            s.max_dd_iid.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into()),
            s.max_tuw_iid_days.map(|d| format!("{:.1}y", d / 365.25)).unwrap_or_else(|| "—".into()),
        )
    } else {
        format!(
            "Survival ({symbol}): ruin {:.0}% · no positive drift (recovery unbounded)",
            s.ruin_prob * 100.0
        )
    })
}

#[cfg(test)]
mod tests {
    use super::survival_summary_text;

    #[test]
    fn survival_summary_none_below_31_closes() {
        let closes: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        assert!(survival_summary_text(&closes, "X").is_none());
    }

    #[test]
    fn survival_summary_text_for_a_drifting_series() {
        // Upward drift with wobble → reliable read with all fields present.
        let closes: Vec<f64> = (0..400)
            .map(|i| {
                let t = i as f64;
                100.0 * (1.0 + 0.0008 * t).max(0.1) + 5.0 * (t / 9.0).sin()
            })
            .collect();
        let s = survival_summary_text(&closes, "BTC").expect("reliable read");
        assert!(s.starts_with("Survival (BTC):"));
        assert!(s.contains("ruin"));
        // Positive-drift series → the reliable branch (has max-DD/underwater).
        assert!(s.contains("max-DD") && s.contains("underwater"));
    }

    #[test]
    fn survival_summary_flags_non_positive_drift() {
        // Downward drift → μ≤0 → the unbounded-recovery branch.
        let closes: Vec<f64> = (0..300).map(|i| 200.0 - 0.3 * i as f64).collect();
        let s = survival_summary_text(&closes, "DOWN").expect("computes");
        assert!(s.contains("no positive drift"));
    }
}
