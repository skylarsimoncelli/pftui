use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::tui::widgets;

/// Render the split-pane detail view for the selected position.
/// Shows chart + recent transactions + news for the selected symbol.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // Draw border around the entire detail pane
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_active))
        .bg(app.theme.surface_1);
    frame.render_widget(block, area);

    // Inner area (inside border)
    let inner = area.inner(Margin::new(1, 1));

    // If no position selected, show placeholder
    let Some(symbol) = &app.selected_symbol else {
        let msg = Paragraph::new("No position selected")
            .style(Style::default().fg(app.theme.text_secondary))
            .alignment(Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    };

    // Split into 3 horizontal sections: chart (50%) + transactions (25%) + news (25%)
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(inner);

    // Chart section
    render_chart_section(frame, h_chunks[0], app, symbol);

    // Transactions section
    render_transactions_section(frame, h_chunks[1], app, symbol);

    // News section
    render_news_section(frame, h_chunks[2], app, symbol);
}

fn render_chart_section(frame: &mut Frame, area: Rect, app: &App, _symbol: &str) {
    // Use the price_chart widget to render the chart for the currently selected position
    widgets::price_chart::render(frame, area, app);
}

fn render_transactions_section(frame: &mut Frame, area: Rect, app: &App, symbol: &str) {
    // Filter transactions for this symbol
    let txs: Vec<_> = app
        .transactions
        .iter()
        .filter(|tx| tx.symbol == symbol)
        .collect();

    if txs.is_empty() {
        let block = Block::default()
            .title("Transactions")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border_inactive))
            .bg(app.theme.surface_1);
        frame.render_widget(block, area);

        let inner = area.inner(Margin::new(1, 1));
        let msg = Paragraph::new("No transactions")
            .style(Style::default().fg(app.theme.text_secondary))
            .alignment(Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    // Build transaction lines (most recent first)
    let mut lines = Vec::new();
    for tx in txs.iter().rev().take(10) {
        let tx_type = match tx.tx_type {
            crate::models::transaction::TxType::Buy => "BUY ",
            crate::models::transaction::TxType::Sell => "SELL",
        };
        let qty = format!("{:.4}", tx.quantity);
        let price = format!("${:.2}", tx.price_per);
        let date = &tx.date[5..10]; // MM-DD

        let line = format!(
            "{} {} {} @ {} {}",
            date, tx_type, qty, price, tx.symbol
        );
        lines.push(Line::from(line).style(Style::default().fg(app.theme.text_primary)));
    }

    let block = Block::default()
        .title(format!("Transactions ({})", symbol))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_inactive))
        .bg(app.theme.surface_1);

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn render_news_section(frame: &mut Frame, area: Rect, app: &App, symbol: &str) {
    // Filter news for this symbol (search for symbol in title)
    let news: Vec<_> = app
        .news_entries
        .iter()
        .filter(|n| n.title.to_lowercase().contains(&symbol.to_lowercase()))
        .collect();

    if news.is_empty() {
        let block = Block::default()
            .title("News")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border_inactive))
            .bg(app.theme.surface_1);
        frame.render_widget(block, area);

        let inner = area.inner(Margin::new(1, 1));
        let msg = Paragraph::new("No recent news")
            .style(Style::default().fg(app.theme.text_secondary))
            .alignment(Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    // Build news lines (most recent first)
    let mut lines = Vec::new();
    for entry in news.iter().take(5) {
        // published_at is i64 (unix timestamp), format it
        let date_str = chrono::DateTime::from_timestamp(entry.published_at, 0)
            .map(|dt| dt.format("%m-%d").to_string())
            .unwrap_or_else(|| "??-??".to_string());
        let title = if entry.title.len() > 30 {
            format!("{}...", &entry.title[..27])
        } else {
            entry.title.clone()
        };
        let line = format!("{} {}", date_str, title);
        lines.push(Line::from(line).style(Style::default().fg(app.theme.text_primary)));
    }

    let block = Block::default()
        .title(format!("News ({})", symbol))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_inactive))
        .bg(app.theme.surface_1);

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}
