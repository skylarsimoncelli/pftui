use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use rusqlite::Connection;

use crate::app::App;
use crate::alerts::engine::check_alerts;
use crate::alerts::AlertStatus;

pub fn render(frame: &mut Frame, app: &App) {
    let t = &app.theme;
    
    // Load alert check results
    let results = if let Ok(conn) = Connection::open(&app.db_path) {
        check_alerts(&conn).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Center popup: 80% width, 80% height
    let area = frame.area();
    let popup_width = (area.width as f32 * 0.8).min(100.0) as u16;
    let popup_height = (area.height as f32 * 0.8).min(30.0) as u16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear background
    let bg = Block::default().style(Style::default().bg(t.surface_0));
    frame.render_widget(bg, popup_area);

    // Popup block
    let title = format!(" Alerts ({}) ", results.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_active))
        .style(Style::default().bg(t.surface_1));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if results.is_empty() {
        let empty_text = Paragraph::new("No alerts configured.\n\nUse `pftui alert add` to create price/allocation/indicator alerts.")
            .style(Style::default().fg(t.text_secondary))
            .alignment(Alignment::Center);
        let empty_area = Rect::new(
            inner.x,
            inner.y + inner.height / 3,
            inner.width,
            3,
        );
        frame.render_widget(empty_text, empty_area);
        return;
    }

    // Build alert list items
    let mut items: Vec<ListItem> = Vec::new();
    
    for result in &results {
        let alert = &result.rule;
        
        // Status icon and color
        let (status_icon, status_color) = match alert.status {
            AlertStatus::Armed => ("🟢", t.gain_green),
            AlertStatus::Triggered => ("🔴", t.loss_red),
            AlertStatus::Acknowledged => ("✅", t.text_muted),
        };

        // Rule text
        let rule_text = &alert.rule_text;
        
        // Current value and distance
        let value_text = if let Some(current) = result.current_value {
            if let Some(dist) = result.distance_pct {
                if alert.status == AlertStatus::Armed {
                    format!("  Current: {} ({:.1}% to trigger)", current, dist)
                } else {
                    format!("  Current: {} (triggered)", current)
                }
            } else {
                format!("  Current: {}", current)
            }
        } else {
            "  Current: N/A".to_string()
        };

        // Build spans for this alert
        let mut spans = vec![
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(format!("[{}]", alert.id), Style::default().fg(t.text_muted)),
            Span::raw(" "),
            Span::styled(rule_text, Style::default().fg(t.text_primary).bold()),
        ];
        
        // Add value info on same line if space permits
        if inner.width > 80 {
            spans.push(Span::styled(value_text, Style::default().fg(t.text_secondary)));
        }

        let line = Line::from(spans);
        items.push(ListItem::new(line));
    }

    // Render scrollable list
    let visible_height = inner.height.saturating_sub(2) as usize; // Leave room for help footer
    let scroll_offset = app.alerts_scroll.min(items.len().saturating_sub(visible_height));
    
    let list_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
    
    // Render with manual offset (since we don't use StatefulWidget)
    let visible_items: Vec<ListItem> = if items.len() > visible_height {
        items.into_iter().skip(scroll_offset).take(visible_height).collect()
    } else {
        items
    };
    
    let visible_list = List::new(visible_items)
        .style(Style::default().bg(t.surface_1));
    frame.render_widget(visible_list, list_area);

    // Help footer
    let help_line = Line::from(vec![
        Span::styled("[j/k]", Style::default().fg(t.key_hint)),
        Span::styled(" Scroll  ", Style::default().fg(t.text_secondary)),
        Span::styled("[Esc]", Style::default().fg(t.key_hint)),
        Span::styled(" Close", Style::default().fg(t.text_secondary)),
    ]);
    let help_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
    let help_widget = Paragraph::new(help_line)
        .alignment(Alignment::Center)
        .style(Style::default().bg(t.surface_2).fg(t.text_secondary));
    frame.render_widget(help_widget, help_area);
}
