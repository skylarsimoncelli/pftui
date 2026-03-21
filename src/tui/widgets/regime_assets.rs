//! Regime Asset Suggestions widget — shows which asset classes are historically
//! strong/weak in the current regime, with portfolio alignment context.
//!
//! Renders as:
//! ```text
//! ┌ Regime Assets ──────────────┐
//! │ 📡 Strong: Gold, Treasuries │
//! │ ⚠  Weak: Crypto, Growth     │
//! │ 60% in regime-favored       │
//! └─────────────────────────────┘
//! ```

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{is_privacy_view, App};
use crate::regime::suggestions::compute_suggestions;
use crate::tui::theme::{self, Theme};

/// Render the regime asset suggestions panel.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let regime = &app.regime_score;

    if !regime.has_data() {
        return;
    }

    let suggestions = compute_suggestions(regime, &app.positions);

    if suggestions.strong.is_empty() && suggestions.weak.is_empty() {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Regime Assets ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width < 10 {
        return;
    }

    let privacy = is_privacy_view(app);
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Strong line
    if !suggestions.strong.is_empty() {
        lines.push(build_asset_line(
            "▲",
            &suggestions.strong,
            t,
            inner.width,
            true,
            privacy,
        ));
    }

    // Weak line
    if !suggestions.weak.is_empty() {
        lines.push(build_asset_line(
            "▼",
            &suggestions.weak,
            t,
            inner.width,
            false,
            privacy,
        ));
    }

    // Alignment line (if we have positions and room)
    if !privacy {
        if let Some(ref alignment) = suggestions.alignment {
            lines.push(build_alignment_line(alignment, t, inner.width));
        }
    }

    // Render lines
    for (i, line) in lines.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect {
                x: inner.x,
                y: inner.y + i as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
}

/// Build a line showing strong or weak assets: "▲ Gold, Treasuries, USD"
fn build_asset_line<'a>(
    icon: &'a str,
    assets: &[String],
    t: &'a Theme,
    max_width: u16,
    is_strong: bool,
    privacy: bool,
) -> Line<'a> {
    let color = if is_strong { t.gain_green } else { t.loss_red };

    if privacy {
        return Line::from(vec![
            Span::styled(format!("{} ", icon), Style::default().fg(color).bold()),
            Span::styled("••••••", Style::default().fg(t.text_muted)),
        ]);
    }

    // Build comma-separated list, truncated to fit
    let prefix = format!("{} ", icon);
    let available = max_width as usize - prefix.len();
    let joined = truncate_list_owned(assets, available);

    Line::from(vec![
        Span::styled(prefix, Style::default().fg(color).bold()),
        Span::styled(joined, Style::default().fg(t.text_secondary)),
    ])
}

/// Build the alignment summary line.
fn build_alignment_line<'a>(
    alignment: &crate::regime::suggestions::PortfolioAlignment,
    t: &'a Theme,
    max_width: u16,
) -> Line<'a> {
    let summary = &alignment.summary;
    let truncated = if summary.len() > max_width as usize {
        format!("{}…", &summary[..max_width as usize - 1])
    } else {
        summary.clone()
    };

    let color = if alignment.strong_pct > alignment.weak_pct {
        t.gain_green
    } else if alignment.weak_pct > alignment.strong_pct {
        t.loss_red
    } else {
        t.text_muted
    };

    Line::from(Span::styled(truncated, Style::default().fg(color).italic()))
}

/// Join items with commas, truncating with "…" if they don't fit.
/// Uses character count (not byte length) for width calculation.
fn truncate_list_owned(items: &[String], max_chars: usize) -> String {
    let mut result = String::new();
    let mut char_count: usize = 0;

    for (i, item) in items.iter().enumerate() {
        let sep = if i > 0 { ", " } else { "" };
        let sep_chars = sep.chars().count();
        let item_chars = item.chars().count();
        let needed = sep_chars + item_chars;

        if char_count + needed > max_chars {
            // Won't fit — add ellipsis if there's room
            if char_count + sep_chars < max_chars {
                result.push_str(sep);
                result.push('…');
            }
            break;
        }

        result.push_str(sep);
        result.push_str(item);
        char_count += needed;
    }
    result
}

/// Compute the appropriate height for the regime assets widget based on data.
pub fn compute_height(app: &App) -> u16 {
    let regime = &app.regime_score;
    if !regime.has_data() {
        return 0;
    }

    let suggestions = compute_suggestions(regime, &app.positions);
    if suggestions.strong.is_empty() && suggestions.weak.is_empty() {
        return 0;
    }

    let mut lines: u16 = 0;
    if !suggestions.strong.is_empty() {
        lines += 1;
    }
    if !suggestions.weak.is_empty() {
        lines += 1;
    }
    if !is_privacy_view(app) && suggestions.alignment.is_some() {
        lines += 1;
    }

    // +2 for borders
    lines + 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn truncate_list_fits() {
        let items = vec!["Gold".to_string(), "Silver".to_string()];
        assert_eq!(super::truncate_list_owned(&items, 20), "Gold, Silver");
    }

    #[test]
    fn truncate_list_overflow() {
        let items = vec![
            "Gold".to_string(),
            "Silver".to_string(),
            "Treasuries".to_string(),
            "USD".to_string(),
            "Utilities".to_string(),
        ];
        let result = super::truncate_list_owned(&items, 25);
        let char_count = result.chars().count();
        assert!(
            char_count <= 25,
            "result too long: {} ({} chars)",
            result,
            char_count
        );
        assert!(result.contains("Gold"));
        // Not all items should fit — at least one is dropped
        assert!(
            !result.contains("Utilities"),
            "all items shouldn't fit in 25 chars: {}",
            result
        );
    }

    #[test]
    fn truncate_list_adds_ellipsis() {
        // With a tighter limit, ellipsis should appear
        let items = vec![
            "Gold".to_string(),
            "Silver".to_string(),
            "Treasuries".to_string(),
        ];
        let result = super::truncate_list_owned(&items, 16);
        let char_count = result.chars().count();
        assert!(
            char_count <= 16,
            "too long: {} ({} chars)",
            result,
            char_count
        );
        assert!(result.contains("Gold"));
        assert!(result.ends_with('…'), "expected ellipsis: {}", result);
    }

    #[test]
    fn truncate_list_empty() {
        let items: Vec<String> = vec![];
        assert_eq!(super::truncate_list_owned(&items, 20), "");
    }

    #[test]
    fn truncate_list_single_item() {
        let items = vec!["Growth stocks".to_string()];
        assert_eq!(super::truncate_list_owned(&items, 20), "Growth stocks");
    }

    #[test]
    fn truncate_list_exact_fit() {
        let items = vec!["Gold".to_string(), "Silver".to_string()];
        // "Gold, Silver" = 12 chars
        assert_eq!(super::truncate_list_owned(&items, 12), "Gold, Silver");
    }

    #[test]
    fn truncate_list_one_over() {
        let items = vec!["Gold".to_string(), "Silver".to_string()];
        // "Gold, Silver" = 12 chars, limit 11 → "Gold, …" or "Gold…"
        let result = super::truncate_list_owned(&items, 11);
        assert!(result.len() <= 11);
        assert!(result.contains("Gold"));
    }
}
