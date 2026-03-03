use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

/// Build a separator line styled with the theme's subtle border color.
fn sep_line(color: Color, width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(color),
    ))
}

/// Build a section header line.
fn section_header(title: &str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default().bold().fg(color),
    ))
}

/// Build a keybinding line: left-aligned key, right-aligned description.
fn key_line(key: &str, desc: &str, key_color: Color, text_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<14}"), Style::default().fg(key_color)),
        Span::styled(desc.to_string(), Style::default().fg(text_color)),
    ])
}

/// Build the full help text content as a list of Lines.
pub fn build_help_lines(app: &App) -> Vec<Line<'static>> {
    let t = &app.theme;
    let sep_w = 48;
    let kc = t.key_hint;
    let tc = t.text_primary;
    let sc = t.text_secondary;
    let ac = t.text_accent;
    let bc = t.border_subtle;

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(64);

    // ── Title ──
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  ◆ ", Style::default().fg(ac)),
        Span::styled("pftui", Style::default().bold().fg(ac)),
        Span::styled(" — Keybindings", Style::default().fg(sc)),
    ]));
    lines.push(Line::from(""));

    // ── Navigation ──
    lines.push(section_header("  Navigation", ac));
    lines.push(sep_line(bc, sep_w));
    lines.push(key_line("j / ↓", "Move down", kc, tc));
    lines.push(key_line("k / ↑", "Move up", kc, tc));
    lines.push(key_line("gg", "Jump to top", kc, tc));
    lines.push(key_line("G", "Jump to bottom", kc, tc));
    lines.push(key_line("Ctrl+d", "Scroll down half page", kc, tc));
    lines.push(key_line("Ctrl+u", "Scroll up half page", kc, tc));
    lines.push(key_line("/", "Search / filter by name", kc, tc));
    lines.push(Line::from(""));

    // ── Views ──
    lines.push(section_header("  Views", ac));
    lines.push(sep_line(bc, sep_w));
    lines.push(key_line("1", "Positions view", kc, tc));
    lines.push(key_line("2", "Transactions view", kc, tc));
    lines.push(key_line("3", "Markets overview", kc, tc));
    lines.push(key_line("4", "Economy dashboard", kc, tc));
    lines.push(key_line("5", "Watchlist", kc, tc));
    lines.push(key_line("Enter", "Position detail / chart", kc, tc));
    lines.push(key_line("Esc", "Close chart / help", kc, tc));
    lines.push(key_line("?", "Toggle this help", kc, tc));
    lines.push(Line::from(""));

    // ── Charts ──
    lines.push(section_header("  Charts", ac));
    lines.push(sep_line(bc, sep_w));
    lines.push(key_line("J / K", "Cycle chart variant", kc, tc));
    lines.push(key_line("h / l", "Cycle chart timeframe (1W–5Y)", kc, tc));
    lines.push(key_line("[ / ]", "Cycle sparkline timeframe (1W–5Y)", kc, tc));
    lines.push(key_line("x", "Toggle crosshair cursor on chart", kc, tc));
    lines.push(Line::from(Span::styled(
        "  Crosshair: h/l move cursor, shows date + price",
        Style::default().fg(sc),
    )));
    lines.push(Line::from(Span::styled(
        "  Variants: Single, Ratio (BTC/SPX …), All",
        Style::default().fg(sc),
    )));
    lines.push(Line::from(Span::styled(
        "  SMA 20/50 overlays on single-symbol charts",
        Style::default().fg(sc),
    )));
    lines.push(Line::from(Span::styled(
        "  Day% column: daily price change percentage",
        Style::default().fg(sc),
    )));
    lines.push(Line::from(Span::styled(
        "  52W column: range bar + distance from 52-week high",
        Style::default().fg(sc),
    )));
    lines.push(Line::from(""));

    // ── Sorting ──
    lines.push(section_header("  Sorting", ac));
    lines.push(sep_line(bc, sep_w));
    lines.push(key_line("a", "Sort by allocation %", kc, tc));
    lines.push(key_line("%", "Sort by gain %", kc, tc));
    lines.push(key_line("$", "Sort by total gain", kc, tc));
    lines.push(key_line("n", "Sort by name", kc, tc));
    lines.push(key_line("c", "Sort by category", kc, tc));
    lines.push(key_line("d", "Sort by date (transactions)", kc, tc));
    lines.push(key_line("Tab", "Toggle ascending / descending", kc, tc));
    lines.push(Line::from(""));

    // ── Actions ──
    lines.push(section_header("  Actions", ac));
    lines.push(sep_line(bc, sep_w));
    lines.push(key_line("f", "Cycle category filter", kc, tc));
    lines.push(key_line("r", "Force refresh prices", kc, tc));
    lines.push(key_line("p", "Toggle privacy view", kc, tc));
    lines.push(key_line("t", "Cycle color theme", kc, tc));
    lines.push(key_line("A (Shift+a)", "Add transaction for position", kc, tc));
    lines.push(key_line("X (Shift+x)", "Delete all txns for position", kc, tc));
    lines.push(key_line("q / Ctrl+C", "Quit", kc, tc));
    lines.push(Line::from(""));

    // ── Footer ──
    lines.push(Line::from(Span::styled(
        "  j/k to scroll · Esc to close",
        Style::default().fg(t.text_muted),
    )));
    lines.push(Line::from(""));

    lines
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let t = &app.theme;

    let help_lines = build_help_lines(app);
    let total_lines = help_lines.len();

    // Popup sizing
    let width = 55u16.min(area.width.saturating_sub(4));
    let height = (total_lines as u16 + 2).min(area.height.saturating_sub(2));
    let visible_lines = height.saturating_sub(2) as usize; // subtract border rows

    // Clamp scroll
    let max_scroll = total_lines.saturating_sub(visible_lines);
    if app.help_scroll > max_scroll {
        app.help_scroll = max_scroll;
    }

    // Apply scroll offset
    let scrolled_lines: Vec<Line> = help_lines
        .into_iter()
        .skip(app.help_scroll)
        .take(visible_lines)
        .collect();

    // Center the popup
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    // Draw shadow behind popup
    crate::tui::theme::render_popup_shadow(frame, popup_area, area, t);

    frame.render_widget(Clear, popup_area);

    // Scroll indicator in title
    let scroll_indicator = if max_scroll > 0 {
        let pct = if max_scroll > 0 {
            (app.help_scroll * 100) / max_scroll
        } else {
            0
        };
        format!(" ◆ Help [{pct}%] ")
    } else {
        " ◆ Help ".to_string()
    };

    let help = Paragraph::new(scrolled_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(crate::tui::theme::BORDER_POPUP)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                scroll_indicator,
                Style::default().fg(t.text_accent).bold(),
            )),
    );

    frame.render_widget(help, popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_app() -> App {
        let config = Config::default();
        let db_path = std::path::PathBuf::from(":memory:");
        App::new(&config, db_path)
    }

    #[test]
    fn help_lines_have_all_sections() {
        let app = test_app();
        let lines = build_help_lines(&app);
        let text: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Navigation"), "missing Navigation section");
        assert!(text.contains("Views"), "missing Views section");
        assert!(text.contains("Charts"), "missing Charts section");
        assert!(text.contains("Sorting"), "missing Sorting section");
        assert!(text.contains("Actions"), "missing Actions section");
    }

    #[test]
    fn help_lines_contain_vim_motions() {
        let app = test_app();
        let lines = build_help_lines(&app);
        let text: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("gg"), "missing gg keybinding");
        assert!(text.contains("Ctrl+d"), "missing Ctrl+d keybinding");
        assert!(text.contains("Ctrl+u"), "missing Ctrl+u keybinding");
        assert!(text.contains("/"), "missing / keybinding");
    }

    #[test]
    fn help_lines_contain_scroll_hint() {
        let app = test_app();
        let lines = build_help_lines(&app);
        let text: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("j/k to scroll"),
            "missing scroll hint in footer"
        );
    }

    #[test]
    fn help_scroll_defaults_to_zero() {
        let app = test_app();
        assert_eq!(app.help_scroll, 0);
    }
}
