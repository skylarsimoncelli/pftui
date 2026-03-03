use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let txs = &app.display_transactions;
    let t = &app.theme;

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
                crate::models::transaction::TxType::Buy => (t.gain_green, "▲ Buy"),
                crate::models::transaction::TxType::Sell => (t.loss_red, "▼ Sell"),
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
                Cell::from(tx.symbol.clone())
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(tx.category.to_string())
                    .style(Style::default().fg(t.category_color(tx.category))),
                Cell::from(type_label).style(Style::default().fg(type_color)),
                Cell::from(format!("{}", tx.quantity))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format!("{:.2}", tx.price_per))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(tx.currency.clone())
                    .style(Style::default().fg(t.text_muted)),
                Cell::from(tx.date.clone())
                    .style(Style::default().fg(t.text_secondary)),
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

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_active))
                .style(Style::default().bg(t.surface_1))
                .title(Span::styled(
                    " Transactions ",
                    Style::default().fg(t.text_primary).bold(),
                )),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}
