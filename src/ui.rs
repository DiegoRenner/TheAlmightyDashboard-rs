use crate::models::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

pub fn render(f: &mut Frame, app: &AppState) {
    let area = f.area();

    // Preserve the original easter egg if window is too small!
    if area.width < 32 || area.height < 8 {
        let msg = Paragraph::new("make biggr pls")
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, area);
        return;
    }

    // Outer layout: Top Header + Main Body
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    // 1. Header
    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " The Almighty Dashboard ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));

    let help_line = Line::from(vec![
        Span::styled("[q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Quit  "),
        Span::styled("[j/↓]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Down  "),
        Span::styled("[k/↑]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Up  "),
        Span::styled(
            format!("(Offset: {})", app.scroll_offset),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let header_widget = Paragraph::new(help_line).block(header_block);
    f.render_widget(header_widget, chunks[0]);

    // 2. Body: Left = Quotes Table, Right = Balances Table
    let body_chunks = if chunks[1].width >= 80 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1])
    };

    render_tickers_table(f, app, body_chunks[0]);
    render_balances_table(f, app, body_chunks[1]);
}

fn render_tickers_table(f: &mut Frame, app: &AppState, area: Rect) {
    let header_cells = ["#", "Symbol", "Price [$]", "Delay [ms]"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows = app
        .tickers
        .iter()
        .enumerate()
        .skip(app.scroll_offset)
        .map(|(idx, item)| {
            let price_style = if item.price_str == "DELISTED" {
                Style::default().fg(Color::Yellow)
            } else if item.price_str == "FAILED" {
                Style::default().fg(Color::Red)
            } else if item.price_str == "unloaded" {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            };

            let sym_style = if item.is_crypto {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default().fg(Color::Blue)
            };

            Row::new(vec![
                Cell::from(format!("{}", idx + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(item.symbol.clone()).style(sym_style),
                Cell::from(item.price_str.clone()).style(price_style),
                Cell::from(format!("{}", item.delay_ms)).style(Style::default().fg(Color::Gray)),
            ])
        });

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(12),
            Constraint::Length(18),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Live Quotes ", Style::default().add_modifier(Modifier::BOLD))),
    );

    f.render_widget(table, area);
}

fn render_balances_table(f: &mut Frame, app: &AppState, area: Rect) {
    let header_cells = ["#", "Symbol", "Amount", "Value [$]"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let mut rows: Vec<Row> = app
        .balances
        .iter()
        .enumerate()
        .skip(app.scroll_offset)
        .map(|(idx, item)| {
            Row::new(vec![
                Cell::from(format!("{}", idx + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(item.symbol.clone()).style(Style::default().fg(Color::Yellow)),
                Cell::from(format_balance_amount(item.amount)).style(Style::default().fg(Color::White)),
                Cell::from(format_balance_value(item.value_usd)).style(Style::default().fg(Color::Green)),
            ])
        })
        .collect();

    // Total row at the bottom
    let total_val = app.total_balance_usd();
    rows.push(
        Row::new(vec![
            Cell::from(""),
            Cell::from("total").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from(""),
            Cell::from(format!("{:.2}", total_val))
                .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    );

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Holdings & Balances ", Style::default().add_modifier(Modifier::BOLD))),
    );

    f.render_widget(table, area);
}

fn format_balance_amount(amount: f64) -> String {
    if amount == 0.0 {
        "0.00".to_string()
    } else if amount >= 0.0001 {
        format!("{:.4}", amount)
    } else {
        format!("{:.8}", amount)
    }
}

fn format_balance_value(val: f64) -> String {
    if val == 0.0 {
        "0.00".to_string()
    } else if val >= 0.01 {
        format!("{:.2}", val)
    } else {
        format!("{:.4}", val)
    }
}
