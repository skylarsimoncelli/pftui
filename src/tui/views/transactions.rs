use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::app::App;
use crate::models::transaction::TxType;

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    if area.width >= 110 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(area);
        render_table(frame, chunks[0], app);
        render_detail_panel(frame, chunks[1], app);
    } else {
        render_table(frame, area, app);
    }
}

fn render_table(frame: &mut Frame, area: Rect, app: &mut App) {
    let txs = &app.display_transactions;
    let t = &app.theme;
    app.page_table_area = Some(area);

    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Symbol"),
        Cell::from("Category"),
        Cell::from("Type"),
        Cell::from("Qty"),
        Cell::from("Price"),
        Cell::from("Ccy"),
        Cell::from("Date"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = txs
        .iter()
        .enumerate()
        .map(|(i, tx)| {
            let (type_color, type_label) = match tx.tx_type {
                TxType::Buy => (t.gain_green, "▲ Buy"),
                TxType::Sell => (t.loss_red, "▼ Sell"),
            };

            let row_bg = if i == app.tx_selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_1_alt
            };

            let style = Style::default().bg(row_bg);
            let marker = if i == app.tx_selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let id_line = Line::from(vec![marker, Span::raw(format!(" {}", tx.id))]);

            Row::new(vec![
                Cell::from(id_line),
                Cell::from(tx.symbol.clone()).style(Style::default().fg(t.text_primary)),
                Cell::from(tx.category.to_string())
                    .style(Style::default().fg(t.category_color(tx.category))),
                Cell::from(type_label).style(Style::default().fg(type_color)),
                Cell::from(format!("{}", tx.quantity)).style(Style::default().fg(t.text_primary)),
                Cell::from(format!("{:.2}", tx.price_per))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(tx.currency.clone()).style(Style::default().fg(t.text_muted)),
                Cell::from(tx.date.clone()).style(Style::default().fg(t.text_secondary)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(5),
        Constraint::Length(12),
    ];

    let filter_suffix = app
        .tx_filter_symbol
        .as_ref()
        .map(|symbol| format!(" [symbol: {symbol}]"))
        .unwrap_or_default();

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_active))
                .style(Style::default().bg(t.surface_1))
                .title(Span::styled(
                    format!(" Transactions{filter_suffix} "),
                    Style::default().fg(t.text_primary).bold(),
                ))
                .title(
                    Line::from(Span::styled(
                        "a:add  x:delete  f:filter  F:clear",
                        Style::default().fg(t.text_muted),
                    ))
                    .alignment(Alignment::Right),
                ),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}

fn render_detail_panel(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Transaction Impact ",
            Style::default().fg(t.text_accent).bold(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(tx) = app.selected_transaction() else {
        frame.render_widget(
            Paragraph::new("No transactions").style(Style::default().fg(t.text_muted)),
            inner,
        );
        return;
    };

    let live_price = app.prices.get(&tx.symbol).copied();
    let market_value = live_price.map(|price| price * tx.quantity);
    let pnl = live_price.map(|price| (price - tx.price_per) * tx.quantity);
    let position = app.positions.iter().find(|pos| pos.symbol == tx.symbol);
    let sibling_count = app
        .transactions
        .iter()
        .filter(|candidate| candidate.symbol == tx.symbol)
        .count();

    let pnl_span = match pnl {
        Some(value) if value > rust_decimal::Decimal::ZERO => {
            Span::styled(format!("{:+.2}", value), Style::default().fg(t.gain_green))
        }
        Some(value) if value < rust_decimal::Decimal::ZERO => {
            Span::styled(format!("{:+.2}", value), Style::default().fg(t.loss_red))
        }
        Some(value) => Span::styled(format!("{:+.2}", value), Style::default().fg(t.text_muted)),
        None => Span::styled("N/A", Style::default().fg(t.text_muted)),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Symbol: ", Style::default().fg(t.text_secondary)),
            Span::styled(&tx.symbol, Style::default().fg(t.text_primary).bold()),
        ]),
        Line::from(vec![
            Span::styled("Type:   ", Style::default().fg(t.text_secondary)),
            Span::styled(
                tx.tx_type.to_string(),
                Style::default().fg(match tx.tx_type {
                    TxType::Buy => t.gain_green,
                    TxType::Sell => t.loss_red,
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Qty:    ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!("{}", tx.quantity),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("Paid:   ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!("{:.2} {}", tx.price_per, tx.currency),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("Live:   ", Style::default().fg(t.text_secondary)),
            Span::styled(
                live_price
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("Value:  ", Style::default().fg(t.text_secondary)),
            Span::styled(
                market_value
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "N/A".to_string()),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("P/L:    ", Style::default().fg(t.text_secondary)),
            pnl_span,
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Related fills: ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!("{sibling_count}"),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("Open position: ", Style::default().fg(t.text_secondary)),
            Span::styled(
                position
                    .map(|pos| format!("{}", pos.quantity))
                    .unwrap_or_else(|| "0".to_string()),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("Cost basis: ", Style::default().fg(t.text_secondary)),
            Span::styled(
                position
                    .map(|pos| format!("{:.2}", pos.avg_cost))
                    .unwrap_or_else(|| "N/A".to_string()),
                Style::default().fg(t.text_primary),
            ),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.surface_0)),
        inner,
    );
}
