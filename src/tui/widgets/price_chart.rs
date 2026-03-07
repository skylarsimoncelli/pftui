use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{App, ChartKind, ChartVariant};
use crate::models::price::HistoryRecord;
use crate::tui::theme;

const BRAILLE_ROWS: usize = 4;

/// Crosshair state passed into chart rendering.
pub struct CrosshairState {
    /// Column index within chart width (clamped by renderer).
    pub x: usize,
}

/// Block characters for volume bars (8 levels of fill)
const VOLUME_BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// SMA periods to overlay on price charts
const SMA_SHORT_PERIOD: usize = 20;
const SMA_LONG_PERIOD: usize = 50;

/// Bollinger Band multiplier (standard deviations from SMA)
const BOLLINGER_MULTIPLIER: f64 = 2.0;

/// Slice history records to only the last `days` entries.
/// Records are assumed to be in chronological order (oldest first).
fn slice_history(records: &[HistoryRecord], days: u32) -> &[HistoryRecord] {
    let n = days as usize;
    if records.len() > n {
        &records[records.len() - n..]
    } else {
        records
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let pos = match app.selected_position() {
        Some(p) => p,
        None => return,
    };

    let variants = App::chart_variants_for_position(pos);
    let variant_count = variants.len();
    let idx = app.chart_index % variant_count.max(1);
    let variant = match variants.into_iter().nth(idx) {
        Some(v) => v,
        None => return,
    };

    // Navigation hint
    let nav_hint = if app.crosshair_mode {
        if variant_count > 1 {
            format!(" ⊹ [{}/{}] J/K  h/l:cursor  x:off ", idx + 1, variant_count)
        } else {
            " ⊹ h/l:cursor  x:off ".to_string()
        }
    } else if variant_count > 1 {
        format!(" [{}/{}] J/K  h/l ", idx + 1, variant_count)
    } else {
        " h/l ".to_string()
    };
    let title = format!(" {} {} ", variant.label, app.chart_timeframe.label());

    let chart_border_color = if app.prices_live {
        // Pulse active chart border when prices are live — subtle breathing effect
        theme::pulse_color(t.border_active, t.border_inactive, app.tick_count, theme::PULSE_PERIOD_BORDER)
    } else {
        t.border_active
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(crate::tui::theme::BORDER_ACTIVE)
        .border_style(Style::default().fg(chart_border_color))
        .style(Style::default().bg(t.surface_1))
        .title(Span::styled(
            title,
            Style::default().fg(t.text_primary).bold(),
        ))
        .title(
            Line::from(Span::styled(
                nav_hint,
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        );

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build crosshair state if active
    let crosshair = if app.crosshair_mode {
        Some(CrosshairState { x: app.crosshair_x })
    } else {
        None
    };

    match &variant.kind {
        ChartKind::All => {
            // Get individual variants (skip index 0 which is "All")
            let all_variants = App::chart_variants_for_position(pos);
            let individuals: Vec<&ChartVariant> = all_variants.iter().skip(1).collect();
            render_multi_panel(frame, inner, &individuals, app);
        }
        ChartKind::Single { symbol, .. } => {
            render_single_chart(frame, inner, symbol, &variant.label, crosshair.as_ref(), app);
        }
        ChartKind::Ratio {
            num_symbol,
            den_symbol,
            ..
        } => {
            render_ratio_chart(frame, inner, num_symbol, den_symbol, &variant.label, crosshair.as_ref(), app);
        }
    }
}

/// Render context header explaining ratio charts
fn render_ratio_context_header(
    frame: &mut Frame,
    area: Rect,
    variants: &[&ChartVariant],
    app: &App,
) {
    let t = &app.theme;
    
    // Detect which asset we're viewing based on the variants
    let primary_symbol = variants.iter().find_map(|v| match &v.kind {
        ChartKind::Single { symbol, .. } => Some(symbol.as_str()),
        _ => None,
    });
    
    let (title, explanation) = match primary_symbol {
        Some("DX-Y.NYB") | Some("DXY") => (
            "Key Macro Ratios",
            "DXY strength vs assets shows dollar purchasing power & safe-haven flows"
        ),
        Some(sym) if sym.contains("GC") || sym.contains("GOLD") => (
            "Gold Context",
            "Gold vs currencies & assets reveals inflation hedging & macro risk sentiment"
        ),
        Some(sym) if sym.contains("BTC") => (
            "Bitcoin Context",
            "BTC vs macro assets tracks risk appetite & digital gold narrative"
        ),
        Some(sym) if sym.starts_with("^") => (
            "Index Context",
            "Index vs macro indicators shows risk-on/risk-off positioning"
        ),
        _ => (
            "Ratio Analysis",
            "Asset relationships reveal relative strength & capital rotation"
        ),
    };
    
    let header_line = Line::from(vec![
        Span::styled(format!(" {} ", title), Style::default().fg(t.text_accent).bold()),
        Span::styled("│ ", Style::default().fg(t.border_inactive)),
        Span::styled(explanation, Style::default().fg(t.text_muted).italic()),
    ]);
    
    let para = Paragraph::new(vec![Line::from(""), header_line])
        .style(Style::default().bg(t.surface_1));
    
    frame.render_widget(para, area);
}

/// Renders a multi-panel stacked view of all individual charts
fn render_multi_panel(
    frame: &mut Frame,
    area: Rect,
    variants: &[&ChartVariant],
    app: &App,
) {
    let t = &app.theme;
    if variants.is_empty() || area.height < 4 {
        return;
    }

    // Check if we have ratio charts and enough height for a header
    let has_ratios = variants.iter().any(|v| matches!(v.kind, ChartKind::Ratio { .. }));
    let header_height = if has_ratios && area.height >= 8 { 2 } else { 0 };
    
    // Render context header if we have ratio charts
    if header_height > 0 {
        let header_area = Rect::new(area.x, area.y, area.width, header_height);
        render_ratio_context_header(frame, header_area, variants, app);
    }

    let chart_area = Rect::new(
        area.x,
        area.y + header_height,
        area.width,
        area.height.saturating_sub(header_height),
    );

    let panel_count = variants.len();
    let panel_height = chart_area.height / panel_count as u16;
    if panel_height < 3 {
        // Too small for multi-panel; just show first (no crosshair in multi-panel)
        if let Some(v) = variants.first() {
            match &v.kind {
                ChartKind::Single { symbol, .. } => {
                    render_single_chart(frame, chart_area, symbol, &v.label, None, app);
                }
                ChartKind::Ratio {
                    num_symbol,
                    den_symbol,
                    ..
                } => {
                    render_ratio_chart(frame, chart_area, num_symbol, den_symbol, &v.label, None, app);
                }
                _ => {}
            }
        }
        return;
    }

    for (i, v) in variants.iter().enumerate() {
        let y = chart_area.y + (i as u16 * panel_height);
        let h = if i == panel_count - 1 {
            chart_area.height - (i as u16 * panel_height)
        } else {
            panel_height
        };
        let panel_area = Rect::new(chart_area.x, y, chart_area.width, h);

        // Label on first line of each panel
        let label_line = Line::from(Span::styled(
            format!(" {} ", v.label),
            Style::default().fg(t.text_accent).bold(),
        ));
        let label_area = Rect::new(panel_area.x, panel_area.y, panel_area.width, 1);
        frame.render_widget(Paragraph::new(label_line), label_area);

        let chart_area = Rect::new(
            panel_area.x,
            panel_area.y + 1,
            panel_area.width,
            panel_area.height.saturating_sub(1),
        );

        match &v.kind {
            ChartKind::Single { symbol, .. } => {
                render_single_mini(frame, chart_area, symbol, app);
            }
            ChartKind::Ratio {
                num_symbol,
                den_symbol,
                ..
            } => {
                render_ratio_mini(frame, chart_area, num_symbol, den_symbol, app);
            }
            _ => {}
        }
    }
}

/// Render a single-symbol chart (full size with stats)
fn render_single_chart(
    frame: &mut Frame,
    area: Rect,
    symbol: &str,
    _label: &str,
    crosshair: Option<&CrosshairState>,
    app: &App,
) {
    let t = &app.theme;
    let tf_days = app.chart_timeframe.days();

    let records = match app.price_history.get(symbol) {
        Some(r) => {
            let sliced = slice_history(r, tf_days);
            if sliced.len() < 2 {
                let msg_text = if app.history_attempted.contains(symbol) {
                    format!("No chart data available for {}", symbol)
                } else {
                    format!("Loading {}...", symbol)
                };
                let msg = Paragraph::new(Span::styled(
                    msg_text,
                    Style::default().fg(t.text_muted),
                ));
                frame.render_widget(msg, area);
                return;
            }
            sliced
        }
        None => {
            let msg_text = if app.history_attempted.contains(symbol) {
                format!("No chart data available for {}", symbol)
            } else {
                format!("Loading {}...", symbol)
            };
            let msg = Paragraph::new(Span::styled(
                msg_text,
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };

    let first_close = records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    // Extract volume data
    let volumes: Vec<Option<u64>> = records.iter().map(|r| r.volume).collect();
    let has_volume = volumes.iter().any(|v| v.is_some());

    // Compute SMA overlays from raw close prices
    let raw_values: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();
    let mut sma_overlays: Vec<(Vec<Option<f64>>, Color)> = Vec::new();

    // Benchmark overlay: when enabled, show SPY normalized to same percentage scale
    if app.benchmark_overlay {
        // Fetch SPY history for same timeframe
        if let Some(spy_records) = app.price_history.get("^GSPC") {
            let spy_sliced = slice_history(spy_records, tf_days);
            if spy_sliced.len() >= 2 {
                let spy_first = spy_sliced.first().map(|r| r.close).unwrap_or(dec!(1));
                let primary_first = records.first().map(|r| r.close).unwrap_or(dec!(1));
                
                // Normalize both to percentage change from first value, then scale SPY to match primary's price scale
                let spy_normalized: Vec<Option<f64>> = spy_sliced
                    .iter()
                    .map(|r| {
                        if spy_first > dec!(0) && primary_first > dec!(0) {
                            let spy_pct_change = (r.close - spy_first) / spy_first;
                            let spy_in_primary_scale = primary_first * (dec!(1) + spy_pct_change);
                            Some(spy_in_primary_scale.to_string().parse::<f64>().unwrap_or(0.0))
                        } else {
                            None
                        }
                    })
                    .collect();
                
                // Add SPY as gray overlay (distinct from indicators)
                sma_overlays.push((spy_normalized, Color::DarkGray));
            }
        }
    }

    let sma20 = compute_sma(&raw_values, SMA_SHORT_PERIOD);
    if sma20.iter().any(|v| v.is_some()) {
        sma_overlays.push((sma20, t.text_accent));
    }
    let sma50 = compute_sma(&raw_values, SMA_LONG_PERIOD);
    if sma50.iter().any(|v| v.is_some()) {
        sma_overlays.push((sma50, t.border_accent));
    }

    let sma_overlay_count = sma_overlays.len();

    // Bollinger Bands: SMA(20) ± 2σ, rendered as faint overlays
    let (bb_upper, bb_lower) = compute_bollinger(&raw_values, SMA_SHORT_PERIOD, BOLLINGER_MULTIPLIER);
    let bb_color = muted_color(t.text_accent, t.surface_1);
    if bb_upper.iter().any(|v| v.is_some()) {
        sma_overlays.push((bb_upper, bb_color));
        sma_overlays.push((bb_lower, bb_color));
    }

    render_braille_chart(frame, area, records, Some(last_close), gain_pct, if has_volume { Some(&volumes) } else { None }, &sma_overlays, sma_overlay_count, crosshair, t);
}

/// Render a ratio chart (numerator / denominator)
fn render_ratio_chart(
    frame: &mut Frame,
    area: Rect,
    num_symbol: &str,
    den_symbol: &str,
    _label: &str,
    crosshair: Option<&CrosshairState>,
    app: &App,
) {
    let t = &app.theme;
    let tf_days = app.chart_timeframe.days();

    let num_records = match app.price_history.get(num_symbol) {
        Some(r) if slice_history(r, tf_days).len() >= 2 => slice_history(r, tf_days),
        _ => {
            let msg_text = if app.history_attempted.contains(num_symbol) {
                format!("No chart data available for {}", num_symbol)
            } else {
                format!("Loading {}...", num_symbol)
            };
            let msg = Paragraph::new(Span::styled(
                msg_text,
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };
    let den_records = match app.price_history.get(den_symbol) {
        Some(r) if slice_history(r, tf_days).len() >= 2 => slice_history(r, tf_days),
        _ => {
            let msg_text = if app.history_attempted.contains(den_symbol) {
                format!("No chart data available for {}", den_symbol)
            } else {
                format!("Loading {}...", den_symbol)
            };
            let msg = Paragraph::new(Span::styled(
                msg_text,
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };

    let ratio_records = compute_ratio(num_records, den_records);
    if ratio_records.len() < 2 {
        let msg = Paragraph::new(Span::styled(
            "Insufficient data for ratio",
            Style::default().fg(t.text_muted),
        ));
        frame.render_widget(msg, area);
        return;
    }

    let first_close = ratio_records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = ratio_records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    // No volume or overlays for ratio charts
    render_braille_chart(frame, area, &ratio_records, Some(last_close), gain_pct, None, &[], 0, crosshair, t);
}

/// Compact single chart for multi-panel (no stats line, just braille)
fn render_single_mini(
    frame: &mut Frame,
    area: Rect,
    symbol: &str,
    app: &App,
) {
    let t = &app.theme;
    let tf_days = app.chart_timeframe.days();
    let records = match app.price_history.get(symbol) {
        Some(r) if slice_history(r, tf_days).len() >= 2 => slice_history(r, tf_days),
        _ => {
            let msg_text = if app.history_attempted.contains(symbol) {
                "No data"
            } else {
                "..."
            };
            let msg = Paragraph::new(Span::styled(
                msg_text,
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };

    let first_close = records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    render_braille_mini(frame, area, records, Some(last_close), gain_pct, t);
}

/// Compact ratio chart for multi-panel
fn render_ratio_mini(
    frame: &mut Frame,
    area: Rect,
    num_symbol: &str,
    den_symbol: &str,
    app: &App,
) {
    let t = &app.theme;
    let tf_days = app.chart_timeframe.days();
    let num_records = match app.price_history.get(num_symbol) {
        Some(r) if slice_history(r, tf_days).len() >= 2 => slice_history(r, tf_days),
        _ => {
            let msg_text = if app.history_attempted.contains(num_symbol) { "No data" } else { "..." };
            let msg = Paragraph::new(Span::styled(msg_text, Style::default().fg(t.text_muted)));
            frame.render_widget(msg, area);
            return;
        }
    };
    let den_records = match app.price_history.get(den_symbol) {
        Some(r) if slice_history(r, tf_days).len() >= 2 => slice_history(r, tf_days),
        _ => {
            let msg_text = if app.history_attempted.contains(den_symbol) { "No data" } else { "..." };
            let msg = Paragraph::new(Span::styled(msg_text, Style::default().fg(t.text_muted)));
            frame.render_widget(msg, area);
            return;
        }
    };

    let ratio_records = compute_ratio(num_records, den_records);
    if ratio_records.len() < 2 {
        let msg = Paragraph::new(Span::styled(
            format!("Loading {}/{}...", num_symbol, den_symbol),
            Style::default().fg(t.text_muted),
        ));
        frame.render_widget(msg, area);
        return;
    }

    let first_close = ratio_records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = ratio_records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    render_braille_mini(frame, area, &ratio_records, Some(last_close), gain_pct, t);
}

/// Compute ratio records by aligning two histories on date and dividing
fn compute_ratio(
    numerator: &[HistoryRecord],
    denominator: &[HistoryRecord],
) -> Vec<HistoryRecord> {
    use std::collections::HashMap;

    let den_map: HashMap<&str, Decimal> = denominator
        .iter()
        .map(|r| (r.date.as_str(), r.close))
        .collect();

    numerator
        .iter()
        .filter_map(|nr| {
            let den_close = den_map.get(nr.date.as_str())?;
            if *den_close > dec!(0) {
                Some(HistoryRecord {
                    date: nr.date.clone(),
                    close: nr.close / *den_close,
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Compute Simple Moving Average for a slice of f64 values.
/// Returns a Vec of the same length as input, where the first `period-1` entries
/// are None (not enough data) and the rest are Some(average).
pub fn compute_sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }
    let mut result = Vec::with_capacity(values.len());
    let mut window_sum = 0.0;
    for (i, &v) in values.iter().enumerate() {
        window_sum += v;
        if i >= period {
            window_sum -= values[i - period];
        }
        if i + 1 >= period {
            result.push(Some(window_sum / period as f64));
        } else {
            result.push(None);
        }
    }
    result
}

/// Compute Bollinger Bands (upper and lower) for a slice of f64 values.
/// Returns (upper_band, lower_band) where each is the same length as input.
/// Bands are SMA(period) ± multiplier * stddev(period).
/// Entries before the first full window are None.
pub fn compute_bollinger(
    values: &[f64],
    period: usize,
    multiplier: f64,
) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    if period == 0 || values.is_empty() {
        let nones = vec![None; values.len()];
        return (nones.clone(), nones);
    }

    let sma = compute_sma(values, period);
    let mut upper = Vec::with_capacity(values.len());
    let mut lower = Vec::with_capacity(values.len());

    for (i, sma_val) in sma.iter().enumerate() {
        match sma_val {
            Some(mean) => {
                // Compute stddev over the window [i+1-period..=i]
                let start = i + 1 - period;
                let window = &values[start..=i];
                let variance = window
                    .iter()
                    .map(|v| {
                        let diff = v - mean;
                        diff * diff
                    })
                    .sum::<f64>()
                    / period as f64;
                let stddev = variance.sqrt();
                upper.push(Some(mean + multiplier * stddev));
                lower.push(Some(mean - multiplier * stddev));
            }
            None => {
                upper.push(None);
                lower.push(None);
            }
        }
    }

    (upper, lower)
}

/// Full braille chart with optional volume bars and stats line (price, gain%, H/L)
/// `sma_count` is the number of SMA overlays (the rest are Bollinger Band overlays).
#[allow(clippy::too_many_arguments)]
fn render_braille_chart(
    frame: &mut Frame,
    area: Rect,
    records: &[HistoryRecord],
    current_price: Option<Decimal>,
    gain_pct: Option<Decimal>,
    volumes: Option<&[Option<u64>]>,
    sma_overlays: &[(Vec<Option<f64>>, Color)],
    sma_count: usize,
    crosshair: Option<&CrosshairState>,
    t: &theme::Theme,
) {
    if area.width < 4 || area.height < 4 {
        return;
    }

    let values: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    // Reserve rows: 1 separator + 1 stats + 1 volume (if available)
    let has_vol = volumes.is_some();
    let reserved_rows: u16 = if has_vol { 4 } else { 3 };
    let chart_height = area.height.saturating_sub(reserved_rows) as usize;
    let chart_width = area.width as usize;

    if chart_height == 0 || chart_width == 0 {
        return;
    }

    let sample_count = chart_width * 2;
    let resampled = resample(&values, sample_count);

    let min_val = resampled.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = resampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_val - min_val;
    let dot_rows = chart_height * BRAILLE_ROWS;

    let normalize_val = |v: f64| -> usize {
        if range > 0.0 {
            (((v - min_val) / range) * (dot_rows.saturating_sub(1)) as f64).round() as usize
        } else {
            dot_rows / 2
        }
    };

    let normalized: Vec<usize> = resampled.iter().map(|v| normalize_val(*v)).collect();

    // Resample and normalize SMA overlays to match the chart grid
    let sma_normalized: Vec<(Vec<Option<usize>>, Color)> = sma_overlays
        .iter()
        .map(|(sma_raw, color)| {
            // Convert Option<f64> to f64 for resampling, preserving None positions
            let sma_f64: Vec<f64> = sma_raw
                .iter()
                .map(|v| v.unwrap_or(f64::NAN))
                .collect();
            let resampled_sma = resample(&sma_f64, sample_count);
            let norm: Vec<Option<usize>> = resampled_sma
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        None
                    } else {
                        Some(normalize_val(*v))
                    }
                })
                .collect();
            (norm, *color)
        })
        .collect();

    let gain = gain_pct.unwrap_or(dec!(0));
    let gain_f: f64 = gain.to_string().parse().unwrap_or(0.0);
    let (grad_low, grad_mid, grad_high) = gain_gradient(gain_f, t);

    let mut lines: Vec<Line> = Vec::new();
    for row in (0..chart_height).rev() {
        let position = if chart_height > 1 {
            row as f32 / (chart_height - 1) as f32
        } else {
            0.5
        };
        let row_color = theme::gradient_3(grad_low, grad_mid, grad_high, position);

        let mut spans = Vec::new();
        for col in 0..chart_width {
            let idx0 = col * 2;
            let idx1 = idx0 + 1;
            let v0 = normalized.get(idx0).copied().unwrap_or(0);
            let v1 = normalized.get(idx1).copied().unwrap_or(0);

            // Compute price braille bits
            let price_bits = braille_bits(v0, v1, row, BRAILLE_ROWS);

            // Compute SMA overlay bits and determine overlay color
            let mut sma_bits: u8 = 0;
            let mut sma_color: Option<Color> = None;
            for (sma_norm, color) in &sma_normalized {
                let sv0 = sma_norm.get(idx0).and_then(|v| *v);
                let sv1 = sma_norm.get(idx1).and_then(|v| *v);
                let bits = braille_dot_bits(sv0, sv1, row, BRAILLE_ROWS);
                if bits != 0 {
                    sma_bits |= bits;
                    sma_color = Some(*color);
                }
            }

            let combined_bits = price_bits | sma_bits;
            let ch = char::from_u32(0x2800 + combined_bits as u32).unwrap_or(' ');

            // Use SMA color if only SMA dots are present, gradient if only price,
            // or gradient if both (price dominates visually)
            let cell_color = if price_bits == 0 && sma_bits != 0 {
                sma_color.unwrap_or(row_color)
            } else {
                row_color
            };

            // Area fill: tint background for cells below the chart line
            let bg_color = area_fill_bg(v0, v1, row, dot_rows, row_color, t.surface_1);

            spans.push(Span::styled(
                String::from(ch),
                Style::default().fg(cell_color).bg(bg_color),
            ));
        }

        // Crosshair: clamp and compute the column position
        let ch_col = crosshair.map(|ch| ch.x.min(chart_width.saturating_sub(1)));

        // Y-axis labels
        let label_width = 6;
        if row == chart_height - 1 && chart_width > label_width + 2 {
            overlay_label(&mut spans, format_compact_short(max_val), t);
        }
        if row == 0 && chart_width > label_width + 2 {
            overlay_label(&mut spans, format_compact_short(min_val), t);
        }

        // Crosshair vertical line overlay
        if let Some(cx) = ch_col {
            if cx < spans.len() {
                spans[cx] = Span::styled("│", Style::default().fg(t.text_accent));
            }
        }

        lines.push(Line::from(spans));
    }

    // Crosshair: compute the record index and data for the tooltip
    let ch_col_clamped = crosshair.map(|ch| ch.x.min(chart_width.saturating_sub(1)));
    let ch_record_data: Option<(&str, f64)> = ch_col_clamped.and_then(|cx| {
        // Map chart column back to source record index
        // Each chart column covers 2 sample points; use the first sample's source position
        let sample_idx = cx * 2;
        if records.is_empty() || sample_count == 0 {
            return None;
        }
        let src_idx_f = (sample_idx as f64 / sample_count as f64) * (records.len() - 1) as f64;
        let src_idx = src_idx_f.round() as usize;
        let src_idx = src_idx.min(records.len() - 1);
        let rec = &records[src_idx];
        let price_f = rec.close.to_string().parse::<f64>().unwrap_or(0.0);
        Some((rec.date.as_str(), price_f))
    });

    // Volume bars (1 row of block characters)
    if let Some(vols) = volumes {
        let mut vol_line = build_volume_line(vols, chart_width, t);
        // Overlay crosshair on volume row too
        if let Some(cx) = ch_col_clamped {
            if cx < vol_line.spans.len() {
                vol_line.spans[cx] = Span::styled("│", Style::default().fg(t.text_accent));
            }
        }
        lines.push(vol_line);
    }

    // Separator
    let mut sep_chars: Vec<Span> = vec![Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(t.border_subtle),
    )];
    // Crosshair marker on separator
    if let Some(cx) = ch_col_clamped {
        // Rebuild separator with crosshair mark
        let mut sep_str: Vec<char> = "─".repeat(area.width as usize).chars().collect();
        if cx < sep_str.len() {
            sep_str[cx] = '┼';
        }
        let sep_string: String = sep_str.into_iter().collect();
        sep_chars = vec![Span::styled(sep_string, Style::default().fg(t.border_subtle))];
    }
    lines.push(Line::from(sep_chars));

    // Stats line: show crosshair data when active, otherwise normal stats
    if let Some((date, price)) = ch_record_data {
        // Crosshair tooltip: date + price at cursor position
        let stats_spans = vec![
            Span::styled("⊹ ", Style::default().fg(t.text_accent)),
            Span::styled(
                date.to_string(),
                Style::default().fg(t.text_primary).bold(),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format_price_f64(price),
                Style::default().fg(t.text_accent).bold(),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                "x:off  h/l:move",
                Style::default().fg(t.text_muted),
            ),
        ];
        lines.push(Line::from(stats_spans));
    } else {
        // Normal price + gain stats line
        let price_str = current_price
            .map(format_price)
            .unwrap_or_else(|| "---".to_string());
        let gain_color = if gain > dec!(0) {
            t.gain_green
        } else if gain < dec!(0) {
            t.loss_red
        } else {
            t.neutral
        };

        let mut stats_spans = vec![
            Span::styled(price_str, Style::default().fg(t.text_primary).bold()),
            Span::raw(" "),
            Span::styled(
                format!("({:+.1}%)", gain),
                Style::default().fg(gain_color),
            ),
            Span::raw("  "),
            Span::styled(
                format!(
                    "H:{} L:{}",
                    format_price_f64(max_val),
                    format_price_f64(min_val)
                ),
                Style::default().fg(t.text_muted),
            ),
        ];

        // Add SMA + Bollinger Band legend labels
        if !sma_overlays.is_empty() {
            stats_spans.push(Span::raw("  "));
            let mut bb_labeled = false;
            for (i, (sma_raw, color)) in sma_overlays.iter().enumerate() {
                if i < sma_count {
                    // SMA overlay — determine period from leading None count
                    let period = sma_raw.iter().take_while(|v| v.is_none()).count() + 1;
                    let label = if period <= SMA_SHORT_PERIOD + 1 {
                        format!("SMA{}", SMA_SHORT_PERIOD)
                    } else {
                        format!("SMA{}", SMA_LONG_PERIOD)
                    };
                    stats_spans.push(Span::styled(
                        format!("─{}", label),
                        Style::default().fg(*color),
                    ));
                } else if !bb_labeled {
                    // Bollinger Band overlay — label once for both upper+lower
                    stats_spans.push(Span::styled(
                        "─BB".to_string(),
                        Style::default().fg(*color),
                    ));
                    bb_labeled = true;
                    // Skip adding separator for the paired lower band
                    continue;
                } else {
                    // Lower band — skip (already labeled)
                    continue;
                }
                if i < sma_overlays.len() - 1 {
                    stats_spans.push(Span::raw(" "));
                }
            }
        }

        lines.push(Line::from(stats_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Build a single line of volume bars using block characters.
/// Resamples volume data to fit the chart width, then maps each column
/// to one of 8 block levels (▁▂▃▄▅▆▇█) based on relative volume.
fn build_volume_line<'a>(
    volumes: &[Option<u64>],
    chart_width: usize,
    t: &theme::Theme,
) -> Line<'a> {
    let vol_f64: Vec<f64> = volumes
        .iter()
        .map(|v| v.unwrap_or(0) as f64)
        .collect();

    let resampled = resample(&vol_f64, chart_width);
    let max_vol = resampled.iter().cloned().fold(0.0_f64, f64::max);

    // Muted color for volume bars — blend toward surface
    let vol_color = muted_color(t.text_muted, t.surface_1);

    let spans: Vec<Span> = resampled
        .iter()
        .map(|&v| {
            if max_vol <= 0.0 || v <= 0.0 {
                Span::styled(" ", Style::default())
            } else {
                let level = ((v / max_vol) * 7.0).round() as usize;
                let ch = VOLUME_BLOCKS[level.min(7)];
                Span::styled(
                    String::from(ch),
                    Style::default().fg(vol_color),
                )
            }
        })
        .collect();

    Line::from(spans)
}

/// Blend two colors to produce a muted intermediate shade
fn muted_color(text: Color, surface: Color) -> Color {
    match (text, surface) {
        (Color::Rgb(tr, tg, tb), Color::Rgb(sr, sg, sb)) => {
            // 60% text, 40% surface for a muted but visible tone
            Color::Rgb(
                ((tr as u16 * 6 + sr as u16 * 4) / 10) as u8,
                ((tg as u16 * 6 + sg as u16 * 4) / 10) as u8,
                ((tb as u16 * 6 + sb as u16 * 4) / 10) as u8,
            )
        }
        _ => text,
    }
}

/// Compact braille chart for multi-panel (1-line stats)
fn render_braille_mini(
    frame: &mut Frame,
    area: Rect,
    records: &[HistoryRecord],
    current_price: Option<Decimal>,
    gain_pct: Option<Decimal>,
    t: &theme::Theme,
) {
    if area.width < 4 || area.height < 2 {
        return;
    }

    let values: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    // Reserve 1 line for stats
    let chart_height = area.height.saturating_sub(1) as usize;
    let chart_width = area.width as usize;

    if chart_height == 0 || chart_width == 0 {
        return;
    }

    let sample_count = chart_width * 2;
    let resampled = resample(&values, sample_count);

    let min_val = resampled.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = resampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_val - min_val;
    let dot_rows = chart_height * BRAILLE_ROWS;

    let normalized: Vec<usize> = resampled
        .iter()
        .map(|v| {
            if range > 0.0 {
                (((v - min_val) / range) * (dot_rows.saturating_sub(1)) as f64).round() as usize
            } else {
                dot_rows / 2
            }
        })
        .collect();

    let gain = gain_pct.unwrap_or(dec!(0));
    let gain_f: f64 = gain.to_string().parse().unwrap_or(0.0);
    let (grad_low, grad_mid, grad_high) = gain_gradient(gain_f, t);

    let mut lines: Vec<Line> = Vec::new();
    for row in (0..chart_height).rev() {
        let position = if chart_height > 1 {
            row as f32 / (chart_height - 1) as f32
        } else {
            0.5
        };
        let row_color = theme::gradient_3(grad_low, grad_mid, grad_high, position);

        let mut spans = Vec::new();
        for col in 0..chart_width {
            let idx0 = col * 2;
            let idx1 = idx0 + 1;
            let v0 = normalized.get(idx0).copied().unwrap_or(0);
            let v1 = normalized.get(idx1).copied().unwrap_or(0);
            let ch = braille_char(v0, v1, row, BRAILLE_ROWS);

            // Area fill: tint background for cells below the chart line
            let bg_color = area_fill_bg(v0, v1, row, dot_rows, row_color, t.surface_1);

            spans.push(Span::styled(
                String::from(ch),
                Style::default().fg(row_color).bg(bg_color),
            ));
        }
        lines.push(Line::from(spans));
    }

    // Compact stats line
    let price_str = current_price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string());
    let gain_color = if gain > dec!(0) {
        t.gain_green
    } else if gain < dec!(0) {
        t.loss_red
    } else {
        t.neutral
    };
    lines.push(Line::from(vec![
        Span::styled(price_str, Style::default().fg(t.text_secondary)),
        Span::raw(" "),
        Span::styled(
            format!("{:+.1}%", gain),
            Style::default().fg(gain_color),
        ),
    ]));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Compute area fill background color for a chart cell.
/// If the cell is below the chart line (both sample points above this row),
/// returns a dim tinted version of the gradient color blended toward the surface.
/// Intensity fades from ~15% near the line to ~5% at the bottom.
/// Returns `surface` unchanged if the cell is at or above the line.
fn area_fill_bg(
    v0: usize,
    v1: usize,
    row: usize,
    total_dot_rows: usize,
    line_color: Color,
    surface: Color,
) -> Color {
    let row_top = (row + 1) * BRAILLE_ROWS;
    let line_max = v0.max(v1);

    // Only fill cells that are entirely below the line
    if line_max < row_top {
        return surface;
    }

    // Compute distance from line to this row (in dot-row units, 0.0 = at line, 1.0 = bottom)
    // line_max is the highest dot position; row_top is the top of this cell
    let distance = if total_dot_rows > 0 {
        1.0 - (row_top as f32 / total_dot_rows as f32)
    } else {
        1.0
    };

    // Opacity fades from FILL_OPACITY_NEAR (near line) to FILL_OPACITY_FAR (bottom)
    let opacity = FILL_OPACITY_FAR + (FILL_OPACITY_NEAR - FILL_OPACITY_FAR) * (1.0 - distance);

    theme::lerp_color(surface, line_color, opacity)
}

/// Maximum area fill opacity (cells just below the line)
const FILL_OPACITY_NEAR: f32 = 0.15;
/// Minimum area fill opacity (cells at the chart bottom)
const FILL_OPACITY_FAR: f32 = 0.04;

fn gain_gradient(gain_f: f64, t: &theme::Theme) -> (Color, Color, Color) {
    if gain_f > 0.0 {
        (
            Color::Rgb(60, 80, 60),
            Color::Rgb(100, 190, 80),
            t.gain_green,
        )
    } else if gain_f < 0.0 {
        (
            t.loss_red,
            Color::Rgb(190, 100, 80),
            Color::Rgb(120, 60, 60),
        )
    } else {
        (t.chart_grad_low, t.chart_grad_mid, t.chart_grad_high)
    }
}

fn overlay_label(spans: &mut [Span], label: String, t: &theme::Theme) {
    for (j, c) in label.chars().enumerate() {
        if j < spans.len() {
            spans[j] = Span::styled(String::from(c), Style::default().fg(t.text_muted));
        }
    }
}

fn resample(values: &[f64], target_len: usize) -> Vec<f64> {
    if values.is_empty() || target_len == 0 {
        return vec![0.0; target_len];
    }
    if values.len() == target_len {
        return values.to_vec();
    }
    let mut result = Vec::with_capacity(target_len);
    for i in 0..target_len {
        let src_idx = (i as f64 / target_len as f64) * (values.len() - 1) as f64;
        let lo = src_idx.floor() as usize;
        let hi = (lo + 1).min(values.len() - 1);
        let frac = src_idx - lo as f64;
        result.push(values[lo] * (1.0 - frac) + values[hi] * frac);
    }
    result
}

/// Compute braille bit pattern for filled-area price line (fills from bottom to value)
fn braille_bits(v0: usize, v1: usize, row: usize, dots_per_row: usize) -> u8 {
    let row_base = row * dots_per_row;
    let mut bits: u8 = 0;

    let col0_bits = [0u8, 1, 2, 6];
    for (dot_idx, &bit) in col0_bits.iter().enumerate() {
        let y = row_base + (dots_per_row - 1 - dot_idx);
        if v0 >= y && y < row_base + dots_per_row {
            bits |= 1 << bit;
        }
    }

    let col1_bits = [3u8, 4, 5, 7];
    for (dot_idx, &bit) in col1_bits.iter().enumerate() {
        let y = row_base + (dots_per_row - 1 - dot_idx);
        if v1 >= y && y < row_base + dots_per_row {
            bits |= 1 << bit;
        }
    }

    bits
}

fn braille_char(v0: usize, v1: usize, row: usize, dots_per_row: usize) -> char {
    let bits = braille_bits(v0, v1, row, dots_per_row);
    char::from_u32(0x2800 + bits as u32).unwrap_or(' ')
}

/// Compute braille bit pattern for a single-dot overlay line (SMA).
/// Unlike braille_bits which fills from bottom, this only lights the dot
/// at exactly the SMA value position — producing a thin line overlay.
fn braille_dot_bits(
    sv0: Option<usize>,
    sv1: Option<usize>,
    row: usize,
    dots_per_row: usize,
) -> u8 {
    let row_base = row * dots_per_row;
    let row_top = row_base + dots_per_row;
    let mut bits: u8 = 0;

    let col0_bits = [0u8, 1, 2, 6];
    if let Some(v) = sv0 {
        if v >= row_base && v < row_top {
            let dot_idx = (dots_per_row - 1) - (v - row_base);
            if dot_idx < col0_bits.len() {
                bits |= 1 << col0_bits[dot_idx];
            }
        }
    }

    let col1_bits = [3u8, 4, 5, 7];
    if let Some(v) = sv1 {
        if v >= row_base && v < row_top {
            let dot_idx = (dots_per_row - 1) - (v - row_base);
            if dot_idx < col1_bits.len() {
                bits |= 1 << col1_bits[dot_idx];
            }
        }
    }

    bits
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    format_price_f64(f)
}

fn format_price_f64(f: f64) -> String {
    if f >= 10000.0 {
        format!("{:.0}", f)
    } else if f >= 100.0 {
        format!("{:.1}", f)
    } else if f >= 1.0 {
        format!("{:.2}", f)
    } else if f >= 0.001 {
        format!("{:.4}", f)
    } else {
        format!("{:.6}", f)
    }
}

fn format_compact_short(f: f64) -> String {
    if f.abs() >= 1_000_000.0 {
        format!("{:.1}M", f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("{:.0}k", f / 1_000.0)
    } else if f.abs() >= 1.0 {
        format!("{:.0}", f)
    } else {
        format!("{:.3}", f)
    }
}

/// Generate braille chart content as `Vec<Line>` for embedding in popups.
/// Takes history records and a theme, renders a compact braille chart with
/// SMA overlays and a stats line. `chart_width` is in terminal columns,
/// `chart_height` is the number of rows to use for the braille area.
/// Returns empty vec if insufficient data.
pub fn render_braille_lines<'a>(
    records: &[HistoryRecord],
    chart_width: usize,
    chart_height: usize,
    t: &'a theme::Theme,
) -> Vec<Line<'a>> {
    if records.len() < 2 || chart_width < 4 || chart_height < 2 {
        return Vec::new();
    }

    let values: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let first_close = records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        ((last_close - first_close) / first_close) * dec!(100)
    } else {
        dec!(0)
    };
    let gain_f: f64 = gain_pct.to_string().parse().unwrap_or(0.0);

    let sample_count = chart_width * 2;
    let resampled = resample(&values, sample_count);

    let min_val = resampled.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = resampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_val - min_val;
    let dot_rows = chart_height * BRAILLE_ROWS;

    let normalize_val = |v: f64| -> usize {
        if range > 0.0 {
            (((v - min_val) / range) * (dot_rows.saturating_sub(1)) as f64).round() as usize
        } else {
            dot_rows / 2
        }
    };

    let normalized: Vec<usize> = resampled.iter().map(|v| normalize_val(*v)).collect();

    // Compute SMA overlays
    let sma20 = compute_sma(&values, SMA_SHORT_PERIOD);
    let sma50 = compute_sma(&values, SMA_LONG_PERIOD);
    let mut sma_overlays: Vec<(Vec<Option<f64>>, Color)> = Vec::new();
    if sma20.iter().any(|v| v.is_some()) {
        sma_overlays.push((sma20, t.text_accent));
    }
    if sma50.iter().any(|v| v.is_some()) {
        sma_overlays.push((sma50, t.border_accent));
    }

    // Resample and normalize SMA overlays
    let sma_normalized: Vec<(Vec<Option<usize>>, Color)> = sma_overlays
        .iter()
        .map(|(sma_raw, color)| {
            let sma_f64: Vec<f64> = sma_raw
                .iter()
                .map(|v| v.unwrap_or(f64::NAN))
                .collect();
            let resampled_sma = resample(&sma_f64, sample_count);
            let norm: Vec<Option<usize>> = resampled_sma
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        None
                    } else {
                        Some(normalize_val(*v))
                    }
                })
                .collect();
            (norm, *color)
        })
        .collect();

    let (grad_low, grad_mid, grad_high) = gain_gradient(gain_f, t);

    let mut lines: Vec<Line> = Vec::new();

    for row in (0..chart_height).rev() {
        let position = if chart_height > 1 {
            row as f32 / (chart_height - 1) as f32
        } else {
            0.5
        };
        let row_color = theme::gradient_3(grad_low, grad_mid, grad_high, position);

        let mut spans = Vec::new();
        // Left padding to match popup text alignment
        spans.push(Span::styled("  ", Style::default()));

        for col in 0..chart_width {
            let idx0 = col * 2;
            let idx1 = idx0 + 1;
            let v0 = normalized.get(idx0).copied().unwrap_or(0);
            let v1 = normalized.get(idx1).copied().unwrap_or(0);

            let price_bits = braille_bits(v0, v1, row, BRAILLE_ROWS);

            let mut sma_bits: u8 = 0;
            let mut sma_color: Option<Color> = None;
            for (sma_norm, color) in &sma_normalized {
                let sv0 = sma_norm.get(idx0).and_then(|v| *v);
                let sv1 = sma_norm.get(idx1).and_then(|v| *v);
                let bits = braille_dot_bits(sv0, sv1, row, BRAILLE_ROWS);
                if bits != 0 {
                    sma_bits |= bits;
                    sma_color = Some(*color);
                }
            }

            let combined_bits = price_bits | sma_bits;
            let ch = char::from_u32(0x2800 + combined_bits as u32).unwrap_or(' ');

            let cell_color = if price_bits == 0 && sma_bits != 0 {
                sma_color.unwrap_or(row_color)
            } else {
                row_color
            };

            let bg_color = area_fill_bg(v0, v1, row, dot_rows, row_color, t.surface_2);

            spans.push(Span::styled(
                String::from(ch),
                Style::default().fg(cell_color).bg(bg_color),
            ));
        }

        // Y-axis labels on first and last rows
        let label_offset = 1; // account for "  " padding at index 0
        if row == chart_height - 1 && chart_width > 8 {
            let label = format_compact_short(max_val);
            for (j, c) in label.chars().enumerate() {
                let span_idx = label_offset + j;
                if span_idx < spans.len() {
                    spans[span_idx] = Span::styled(String::from(c), Style::default().fg(t.text_muted));
                }
            }
        }
        if row == 0 && chart_width > 8 {
            let label = format_compact_short(min_val);
            for (j, c) in label.chars().enumerate() {
                let span_idx = label_offset + j;
                if span_idx < spans.len() {
                    spans[span_idx] = Span::styled(String::from(c), Style::default().fg(t.text_muted));
                }
            }
        }

        lines.push(Line::from(spans));
    }

    // Stats line
    let price_str = format_price(last_close);
    let gain_color = if gain_pct > dec!(0) {
        t.gain_green
    } else if gain_pct < dec!(0) {
        t.loss_red
    } else {
        t.neutral
    };

    let mut stats_spans = vec![
        Span::styled("  ", Style::default()),
        Span::styled(price_str, Style::default().fg(t.text_primary).bold()),
        Span::raw(" "),
        Span::styled(
            format!("({:+.1}%)", gain_pct),
            Style::default().fg(gain_color),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "H:{} L:{}",
                format_price_f64(max_val),
                format_price_f64(min_val)
            ),
            Style::default().fg(t.text_muted),
        ),
    ];

    // SMA legend
    if !sma_overlays.is_empty() {
        stats_spans.push(Span::raw("  "));
        for (i, (_sma_raw, color)) in sma_overlays.iter().enumerate() {
            let label = if i == 0 {
                format!("─SMA{}", SMA_SHORT_PERIOD)
            } else {
                format!("─SMA{}", SMA_LONG_PERIOD)
            };
            stats_spans.push(Span::styled(label, Style::default().fg(*color)));
            if i < sma_overlays.len() - 1 {
                stats_spans.push(Span::raw(" "));
            }
        }
    }

    lines.push(Line::from(stats_spans));

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_volume_blocks_levels() {
        assert_eq!(VOLUME_BLOCKS.len(), 8);
        assert_eq!(VOLUME_BLOCKS[0], '▁');
        assert_eq!(VOLUME_BLOCKS[7], '█');
    }

    #[test]
    fn test_build_volume_line_all_zero() {
        let volumes: Vec<Option<u64>> = vec![None, None, None, None, None];
        let t = theme::midnight();
        let line = build_volume_line(&volumes, 5, &t);
        // All spaces when no volume data
        for span in line.spans.iter() {
            assert_eq!(span.content.as_ref(), " ");
        }
    }

    #[test]
    fn test_build_volume_line_scaling() {
        let volumes: Vec<Option<u64>> = vec![
            Some(100),
            Some(500),
            Some(1000),
            Some(750),
            Some(250),
        ];
        let t = theme::midnight();
        let line = build_volume_line(&volumes, 5, &t);
        // Max volume (1000) should be █ (level 7), min non-zero should be ▁ (level 0-1)
        assert_eq!(line.spans.len(), 5);
        assert_eq!(line.spans[2].content.as_ref(), "█"); // 1000/1000 = max
    }

    #[test]
    fn test_build_volume_line_resamples() {
        let volumes: Vec<Option<u64>> = vec![Some(100), Some(500)];
        let t = theme::midnight();
        let line = build_volume_line(&volumes, 10, &t);
        // Should produce 10 spans from 2 data points
        assert_eq!(line.spans.len(), 10);
    }

    #[test]
    fn test_compute_ratio_has_no_volume() {
        let num = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(500_000), open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(200), volume: Some(600_000), open: None, high: None, low: None },
        ];
        let den = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(50), volume: Some(300_000), open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(100), volume: Some(400_000), open: None, high: None, low: None },
        ];
        let result = compute_ratio(&num, &den);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].close, dec!(2));
        assert!(result[0].volume.is_none());
        assert!(result[1].volume.is_none());
    }

    #[test]
    fn test_muted_color_blends() {
        let text = Color::Rgb(200, 200, 200);
        let surface = Color::Rgb(20, 20, 20);
        let result = muted_color(text, surface);
        // 200*0.6 + 20*0.4 = 120 + 8 = 128
        assert_eq!(result, Color::Rgb(128, 128, 128));
    }

    #[test]
    fn test_muted_color_non_rgb_passthrough() {
        let result = muted_color(Color::White, Color::Black);
        assert_eq!(result, Color::White);
    }

    #[test]
    fn test_compute_sma_basic() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let sma = compute_sma(&values, 3);
        assert_eq!(sma.len(), 5);
        // First 2 are None (need 3 values for period 3)
        assert!(sma[0].is_none());
        assert!(sma[1].is_none());
        // SMA(3) at index 2: (10+20+30)/3 = 20.0
        assert!((sma[2].unwrap() - 20.0).abs() < 1e-10);
        // SMA(3) at index 3: (20+30+40)/3 = 30.0
        assert!((sma[3].unwrap() - 30.0).abs() < 1e-10);
        // SMA(3) at index 4: (30+40+50)/3 = 40.0
        assert!((sma[4].unwrap() - 40.0).abs() < 1e-10);
    }

    #[test]
    fn test_compute_sma_period_1() {
        let values = vec![5.0, 10.0, 15.0];
        let sma = compute_sma(&values, 1);
        // SMA(1) = the value itself
        assert!((sma[0].unwrap() - 5.0).abs() < 1e-10);
        assert!((sma[1].unwrap() - 10.0).abs() < 1e-10);
        assert!((sma[2].unwrap() - 15.0).abs() < 1e-10);
    }

    #[test]
    fn test_compute_sma_period_zero() {
        let values = vec![1.0, 2.0, 3.0];
        let sma = compute_sma(&values, 0);
        assert_eq!(sma.len(), 3);
        assert!(sma.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_compute_sma_empty_input() {
        let values: Vec<f64> = vec![];
        let sma = compute_sma(&values, 5);
        assert!(sma.is_empty());
    }

    #[test]
    fn test_compute_sma_period_larger_than_data() {
        let values = vec![1.0, 2.0, 3.0];
        let sma = compute_sma(&values, 5);
        assert_eq!(sma.len(), 3);
        // All None since we never have 5 data points
        assert!(sma.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_braille_dot_bits_single_dot() {
        // A dot at position 2 in row 0 (rows span positions 0-3)
        // dot_idx = 3 - (2 - 0) = 1 -> col0_bits[1] = bit 1
        let bits = braille_dot_bits(Some(2), None, 0, BRAILLE_ROWS);
        assert_ne!(bits, 0);
    }

    #[test]
    fn test_braille_dot_bits_no_dot_outside_row() {
        // Dot at position 5, row 0 spans 0-3 → should be empty
        let bits = braille_dot_bits(Some(5), None, 0, BRAILLE_ROWS);
        assert_eq!(bits, 0);
    }

    #[test]
    fn test_braille_dot_bits_both_columns() {
        // Dots in both columns at position 1, row 0
        let bits = braille_dot_bits(Some(1), Some(1), 0, BRAILLE_ROWS);
        // Both columns should have a dot → non-zero bits from both col0 and col1
        assert_ne!(bits, 0);
        // Verify it has bits from both columns
        let col0_only = braille_dot_bits(Some(1), None, 0, BRAILLE_ROWS);
        let col1_only = braille_dot_bits(None, Some(1), 0, BRAILLE_ROWS);
        assert_eq!(bits, col0_only | col1_only);
    }

    #[test]
    fn test_braille_dot_bits_none_is_empty() {
        let bits = braille_dot_bits(None, None, 0, BRAILLE_ROWS);
        assert_eq!(bits, 0);
    }

    #[test]
    fn test_compute_ratio_basic() {
        let num = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(200), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(150), volume: None, open: None, high: None, low: None },
        ];
        let den = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(50), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(75), volume: None, open: None, high: None, low: None },
        ];
        let result = compute_ratio(&num, &den);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].close, dec!(2));
        assert_eq!(result[1].close, dec!(2));
        assert_eq!(result[2].close, dec!(2));
    }

    #[test]
    fn test_compute_ratio_skips_missing_dates() {
        let num = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(200), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(300), volume: None, open: None, high: None, low: None },
        ];
        let den = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(50), volume: None, open: None, high: None, low: None },
            // no 2025-01-02
            HistoryRecord { date: "2025-01-03".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
        ];
        let result = compute_ratio(&num, &den);
        assert_eq!(result.len(), 2); // only matching dates
        assert_eq!(result[0].date, "2025-01-01");
        assert_eq!(result[1].date, "2025-01-03");
    }

    #[test]
    fn test_compute_ratio_skips_zero_denominator() {
        let num = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
        ];
        let den = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(0), volume: None, open: None, high: None, low: None },
        ];
        let result = compute_ratio(&num, &den);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_compute_ratio_empty_inputs() {
        let empty: Vec<HistoryRecord> = vec![];
        let non_empty = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
        ];
        assert!(compute_ratio(&empty, &non_empty).is_empty());
        assert!(compute_ratio(&non_empty, &empty).is_empty());
        assert!(compute_ratio(&empty, &empty).is_empty());
    }

    #[test]
    fn test_resample_identity() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = resample(&values, 5);
        assert_eq!(result, values);
    }

    #[test]
    fn test_resample_upscale() {
        let values = vec![0.0, 10.0];
        let result = resample(&values, 5);
        assert_eq!(result.len(), 5);
        // Maps i/target_len across source range: 0.0, 2.0, 4.0, 6.0, 8.0
        assert!((result[0] - 0.0).abs() < 1e-10);
        assert!((result[1] - 2.0).abs() < 1e-10);
        assert!((result[2] - 4.0).abs() < 1e-10);
        assert!((result[3] - 6.0).abs() < 1e-10);
        assert!((result[4] - 8.0).abs() < 1e-10);
    }

    #[test]
    fn test_resample_downscale() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let result = resample(&values, 3);
        assert_eq!(result.len(), 3);
        // Maps i/3 across 0..9: src_idx = 0.0, 3.0, 6.0 → values 1.0, 4.0, 7.0
        assert!((result[0] - 1.0).abs() < 1e-10);
        assert!((result[1] - 4.0).abs() < 1e-10);
        assert!((result[2] - 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_resample_empty_input() {
        let result = resample(&[], 5);
        assert_eq!(result.len(), 5);
        assert!(result.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_resample_zero_target() {
        let values = vec![1.0, 2.0, 3.0];
        let result = resample(&values, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_resample_single_value() {
        let values = vec![42.0];
        let result = resample(&values, 5);
        assert_eq!(result.len(), 5);
        // All should be 42.0 (interpolating between same point)
        assert!(result.iter().all(|&v| (v - 42.0).abs() < 1e-10));
    }

    #[test]
    fn test_crosshair_state_clamps_to_chart_width() {
        // CrosshairState with x beyond chart width should be clamped during render
        let ch = CrosshairState { x: 1000 };
        let chart_width: usize = 50;
        let clamped = ch.x.min(chart_width.saturating_sub(1));
        assert_eq!(clamped, 49);
    }

    #[test]
    fn test_crosshair_state_zero_is_valid() {
        let ch = CrosshairState { x: 0 };
        let chart_width: usize = 50;
        let clamped = ch.x.min(chart_width.saturating_sub(1));
        assert_eq!(clamped, 0);
    }

    #[test]
    fn test_crosshair_record_mapping() {
        // Given a chart width of 10, sample_count = 20,
        // and 5 records, crosshair at column 5 should map to record ~2
        let records = [HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(120), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-04".into(), close: dec!(130), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-05".into(), close: dec!(140), volume: None, open: None, high: None, low: None }];
        let chart_width = 10;
        let sample_count = chart_width * 2; // 20
        let crosshair_col = 5;
        let sample_idx = crosshair_col * 2; // 10
        let src_idx_f = (sample_idx as f64 / sample_count as f64) * (records.len() - 1) as f64;
        let src_idx = src_idx_f.round() as usize;
        let src_idx = src_idx.min(records.len() - 1);
        assert_eq!(records[src_idx].date, "2025-01-03");
    }

    #[test]
    fn test_crosshair_record_mapping_rightmost() {
        let records = [HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(200), volume: None, open: None, high: None, low: None }];
        let chart_width = 10;
        let sample_count = chart_width * 2;
        let crosshair_col = chart_width - 1; // rightmost
        let sample_idx = crosshair_col * 2;
        let src_idx_f = (sample_idx as f64 / sample_count as f64) * (records.len() - 1) as f64;
        let src_idx = (src_idx_f.round() as usize).min(records.len() - 1);
        assert_eq!(records[src_idx].date, "2025-01-02");
    }

    #[test]
    fn test_crosshair_record_mapping_leftmost() {
        let records = [HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(200), volume: None, open: None, high: None, low: None }];
        let chart_width = 10;
        let sample_count = chart_width * 2;
        let crosshair_col = 0; // leftmost
        let sample_idx = crosshair_col * 2;
        let src_idx_f = (sample_idx as f64 / sample_count as f64) * (records.len() - 1) as f64;
        let src_idx = (src_idx_f.round() as usize).min(records.len() - 1);
        assert_eq!(records[src_idx].date, "2025-01-01");
    }

    #[test]
    fn test_area_fill_bg_returns_surface_when_above_line() {
        let surface = Color::Rgb(20, 20, 30);
        let line_color = Color::Rgb(100, 200, 100);
        // v0=2, v1=2 means line is at dot position 2 (row 0 spans dots 0-3)
        // row 1 spans dots 4-7, which is above the line → should return surface
        let result = area_fill_bg(2, 2, 1, 8, line_color, surface);
        assert_eq!(result, surface);
    }

    #[test]
    fn test_area_fill_bg_tints_when_below_line() {
        let surface = Color::Rgb(20, 20, 30);
        let line_color = Color::Rgb(100, 200, 100);
        // v0=7, v1=7 means line at top of row 1 (dot 7)
        // row 0 spans dots 0-3, row_top=4, line_max=7 → 7 >= 4 → fill
        let result = area_fill_bg(7, 7, 0, 8, line_color, surface);
        assert_ne!(result, surface);
        // Should be a blend between surface and line_color
        if let Color::Rgb(r, g, b) = result {
            // The fill should shift colors toward line_color
            assert!(r > 20 || g > 20 || b > 30);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_area_fill_bg_bottom_row_dimmer_than_near_line() {
        let surface = Color::Rgb(20, 20, 30);
        let line_color = Color::Rgb(200, 200, 200);
        // Line at top (dot 15 in a 16-dot grid = 4 rows * 4 dots)
        // row 0 is bottom, row 2 is near the line
        let bottom = area_fill_bg(15, 15, 0, 16, line_color, surface);
        let near_line = area_fill_bg(15, 15, 2, 16, line_color, surface);
        // Near-line should be brighter (more shifted toward line_color)
        if let (Color::Rgb(br, _, _), Color::Rgb(nr, _, _)) = (bottom, near_line) {
            assert!(nr >= br, "Near-line fill ({}) should be >= bottom fill ({})", nr, br);
        } else {
            panic!("Expected Rgb colors");
        }
    }

    #[test]
    fn test_area_fill_bg_uses_max_of_v0_v1() {
        let surface = Color::Rgb(20, 20, 30);
        let line_color = Color::Rgb(100, 200, 100);
        // v0=1, v1=6 → max is 6, row 0 top is 4, 6 >= 4 → fill
        let result = area_fill_bg(1, 6, 0, 8, line_color, surface);
        assert_ne!(result, surface);
    }

    #[test]
    fn test_area_fill_bg_exact_boundary_no_fill() {
        let surface = Color::Rgb(20, 20, 30);
        let line_color = Color::Rgb(100, 200, 100);
        // row 0, row_top = 4. line_max = 3 → 3 < 4 → no fill
        let result = area_fill_bg(3, 3, 0, 8, line_color, surface);
        assert_eq!(result, surface);
    }

    #[test]
    fn test_area_fill_opacity_constants() {
        const { assert!(FILL_OPACITY_NEAR > FILL_OPACITY_FAR) };
        const { assert!(FILL_OPACITY_NEAR <= 0.20) };
        const { assert!(FILL_OPACITY_FAR >= 0.0) };
    }

    #[test]
    fn test_compute_bollinger_basic() {
        // 5 values, period 3, multiplier 2.0
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let (upper, lower) = compute_bollinger(&values, 3, 2.0);
        assert_eq!(upper.len(), 5);
        assert_eq!(lower.len(), 5);
        // First 2 are None (need 3 values for period 3)
        assert!(upper[0].is_none());
        assert!(upper[1].is_none());
        assert!(lower[0].is_none());
        assert!(lower[1].is_none());
        // At index 2: SMA = 20.0, window = [10, 20, 30]
        // variance = ((10-20)^2 + (20-20)^2 + (30-20)^2) / 3 = (100+0+100)/3 = 66.67
        // stddev = sqrt(66.67) ≈ 8.165
        // upper = 20.0 + 2*8.165 ≈ 36.33, lower = 20.0 - 2*8.165 ≈ 3.67
        let u2 = upper[2].unwrap();
        let l2 = lower[2].unwrap();
        assert!((u2 - 36.33).abs() < 0.1, "upper[2] = {}", u2);
        assert!((l2 - 3.67).abs() < 0.1, "lower[2] = {}", l2);
    }

    #[test]
    fn test_compute_bollinger_symmetric_around_sma() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let (upper, lower) = compute_bollinger(&values, 3, 2.0);
        let sma = compute_sma(&values, 3);
        // For each defined point, upper and lower should be equidistant from SMA
        for i in 0..values.len() {
            if let (Some(u), Some(l), Some(m)) = (upper[i], lower[i], sma[i]) {
                let diff_upper = u - m;
                let diff_lower = m - l;
                assert!(
                    (diff_upper - diff_lower).abs() < 1e-10,
                    "Bands not symmetric at index {}: upper_diff={}, lower_diff={}",
                    i, diff_upper, diff_lower
                );
            }
        }
    }

    #[test]
    fn test_compute_bollinger_constant_values_zero_bandwidth() {
        // If all values are the same, stddev = 0, so bands collapse to the SMA
        let values = vec![50.0, 50.0, 50.0, 50.0, 50.0];
        let (upper, lower) = compute_bollinger(&values, 3, 2.0);
        for i in 2..5 {
            assert!((upper[i].unwrap() - 50.0).abs() < 1e-10);
            assert!((lower[i].unwrap() - 50.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_compute_bollinger_empty_input() {
        let values: Vec<f64> = vec![];
        let (upper, lower) = compute_bollinger(&values, 20, 2.0);
        assert!(upper.is_empty());
        assert!(lower.is_empty());
    }

    #[test]
    fn test_compute_bollinger_period_zero() {
        let values = vec![1.0, 2.0, 3.0];
        let (upper, lower) = compute_bollinger(&values, 0, 2.0);
        assert_eq!(upper.len(), 3);
        assert!(upper.iter().all(|v| v.is_none()));
        assert!(lower.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_compute_bollinger_period_larger_than_data() {
        let values = vec![1.0, 2.0, 3.0];
        let (upper, lower) = compute_bollinger(&values, 10, 2.0);
        assert_eq!(upper.len(), 3);
        // All None since we never have 10 data points
        assert!(upper.iter().all(|v| v.is_none()));
        assert!(lower.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_compute_bollinger_upper_above_lower() {
        // With positive multiplier, upper should always be >= lower
        let values = vec![10.0, 15.0, 8.0, 22.0, 18.0, 5.0, 30.0];
        let (upper, lower) = compute_bollinger(&values, 3, 2.0);
        for i in 0..values.len() {
            if let (Some(u), Some(l)) = (upper[i], lower[i]) {
                assert!(u >= l, "upper ({}) < lower ({}) at index {}", u, l, i);
            }
        }
    }

    #[test]
    fn test_bollinger_multiplier_constant() {
        assert!((BOLLINGER_MULTIPLIER - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_render_braille_lines_insufficient_data() {
        let t = theme::midnight();
        let records = vec![HistoryRecord {
            date: "2026-01-01".into(),
            close: dec!(100),
            volume: None,
                open: None,
                high: None,
                low: None,
            }];
        let lines = render_braille_lines(&records, 40, 6, &t);
        assert!(lines.is_empty(), "Should return empty for < 2 records");
    }

    #[test]
    fn test_render_braille_lines_too_narrow() {
        let t = theme::midnight();
        let records = vec![
            HistoryRecord { date: "2026-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-02".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
        ];
        let lines = render_braille_lines(&records, 2, 6, &t);
        assert!(lines.is_empty(), "Should return empty for width < 4");
    }

    #[test]
    fn test_render_braille_lines_basic_structure() {
        let t = theme::midnight();
        let mut records = Vec::new();
        for i in 0..30 {
            records.push(HistoryRecord {
                date: format!("2026-01-{:02}", (i % 28) + 1),
                close: dec!(100) + Decimal::from(i),
                volume: None,
                open: None,
                high: None,
                low: None,
            });
        }
        let chart_height = 6;
        let lines = render_braille_lines(&records, 40, chart_height, &t);
        // Should have chart_height braille rows + 1 stats line
        assert_eq!(lines.len(), chart_height + 1, "Expected {} lines, got {}", chart_height + 1, lines.len());
    }

    #[test]
    fn test_render_braille_lines_stats_line_contains_price() {
        let t = theme::midnight();
        let records = vec![
            HistoryRecord { date: "2026-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-02".into(), close: dec!(120), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-03".into(), close: dec!(130), volume: None, open: None, high: None, low: None },
        ];
        let lines = render_braille_lines(&records, 40, 4, &t);
        // Last line is the stats line, should contain the last price
        let stats_text: String = lines.last().unwrap().spans.iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(stats_text.contains("130"), "Stats line should contain last price: {}", stats_text);
    }

    #[test]
    fn test_render_braille_lines_has_left_padding() {
        let t = theme::midnight();
        let records = vec![
            HistoryRecord { date: "2026-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-02".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
        ];
        let lines = render_braille_lines(&records, 20, 4, &t);
        // First span of each braille row should be "  " (left padding)
        for line in &lines[..lines.len() - 1] {
            assert_eq!(line.spans[0].content.as_ref(), "  ", "Braille rows should have 2-space left padding");
        }
    }

    #[test]
    fn test_render_braille_lines_with_sma_data() {
        let t = theme::midnight();
        // Need 50+ records for SMA(50) to kick in
        let mut records = Vec::new();
        for i in 0..60 {
            records.push(HistoryRecord {
                date: format!("2026-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1),
                close: dec!(100) + Decimal::from(i),
                volume: None,
                open: None,
                high: None,
                low: None,
            });
        }
        let lines = render_braille_lines(&records, 50, 6, &t);
        // Stats line should contain SMA legend
        let stats_text: String = lines.last().unwrap().spans.iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(stats_text.contains("SMA20"), "Stats should show SMA20 legend: {}", stats_text);
        assert!(stats_text.contains("SMA50"), "Stats should show SMA50 legend: {}", stats_text);
    }

}
