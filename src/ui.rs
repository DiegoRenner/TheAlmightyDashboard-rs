use crate::models::{AccountCategory, AppState};
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
            format!("(Offset: {}) ", app.scroll_offset),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!(
                "| FX: USD/CHF {:.3} · EUR/CHF {:.3} · GBP/CHF {:.3}",
                app.fx_rates.usd_to_chf, app.fx_rates.eur_to_chf, app.fx_rates.gbp_to_chf
            ),
            Style::default().fg(Color::Cyan),
        ),
    ]);

    let header_widget = Paragraph::new(help_line).block(header_block);
    f.render_widget(header_widget, chunks[0]);

    // 2. Body: Left = Quotes Table, Right = Balances Table & Portfolio Summary
    let body_chunks = if chunks[1].width >= 80 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1])
    };

    render_tickers_table(f, app, body_chunks[0]);
    render_balances_panel(f, app, body_chunks[1]);
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
            Constraint::Min(10),
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

fn render_balances_panel(f: &mut Frame, app: &AppState, area: Rect) {
    if area.height < 12 {
        render_balances_table(f, app, area);
        return;
    }

    let sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(6)])
        .split(area);

    render_balances_table(f, app, sub_chunks[0]);
    render_summary_card(f, app, sub_chunks[1]);
}

fn render_balances_table(f: &mut Frame, app: &AppState, area: Rect) {
    let header_cells = ["#", "Acc", "Cat", "Asset", "Amount", "Val (CHF)"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = app
        .balances
        .iter()
        .enumerate()
        .skip(app.scroll_offset)
        .map(|(idx, item)| {
            let cat_style = match item.category {
                AccountCategory::Crypto => Style::default().fg(Color::Magenta),
                AccountCategory::Stocks => Style::default().fg(Color::Blue),
                AccountCategory::Cash => Style::default().fg(Color::Cyan),
                AccountCategory::Retirement => Style::default().fg(Color::Green),
            };

            let is_session = AppState::is_session_dependent(&item.account);
            let is_stale = app.is_account_stale(&item.account);

            let acc_label = if is_session {
                format!("{}*", item.account)
            } else {
                item.account.clone()
            };

            let acc_style = if is_stale {
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
            } else if is_session {
                Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            };

            Row::new(vec![
                Cell::from(format!("{}", idx + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(acc_label).style(acc_style),
                Cell::from(item.category.to_string()).style(cat_style),
                Cell::from(item.symbol.clone()).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Cell::from(format_balance_amount(item.amount)).style(Style::default().fg(Color::White)),
                Cell::from(format_chf(item.value_chf)).style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(14),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![
                Span::styled(" Holdings & Accounts ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("(", Style::default().fg(Color::DarkGray)),
                Span::styled("● API", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled("●* Session", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled("● Stale", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
                Span::styled(") ", Style::default().fg(Color::DarkGray)),
            ]))
            .title_bottom(Line::from(vec![
                Span::styled(" Categories: ", Style::default().fg(Color::DarkGray)),
                Span::styled("● Stocks", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled("● Cash", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled("● Crypto", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled("● Retirement", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
            ])),
    );

    f.render_widget(table, area);
}

fn render_summary_card(f: &mut Frame, app: &AppState, area: Rect) {
    let stocks_val = app.stocks_total_chf();
    let cash_val = app.cash_total_chf();
    let stocks_cash_val = app.stocks_and_cash_total_chf();
    let crypto_val = app.crypto_total_chf();
    let ret_val = app.retirement_total_chf();
    let total_net_worth = app.total_net_worth_chf();
    let total_usd = app.total_balance_usd();

    let is_fp_stale = app.is_account_stale("FP");
    let mut ret_spans = vec![
        Span::styled(
            " Retirement:    ",
            Style::default()
                .fg(if is_fp_stale { Color::LightRed } else { Color::Green })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("CHF {}", format_chf(ret_val)),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ];
    if is_fp_stale {
        ret_spans.push(Span::styled(" (stale session)", Style::default().fg(Color::LightRed)));
    }

    let summary_lines = vec![
        Line::from(vec![
            Span::styled(" Stocks & Cash: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("CHF {} ", format_chf(stocks_cash_val)), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("(Stocks: {} / Cash: {})", format_chf(stocks_val), format_chf(cash_val)), Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled(" Crypto:        ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(format!("CHF {}", format_chf(crypto_val)), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(ret_spans),
        Line::from(vec![
            Span::styled(" Total (CHF):   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!("CHF {} ", format_chf(total_net_worth)), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(format!("(~${} USD)", format_chf(total_usd)), Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let summary_widget = Paragraph::new(summary_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(" Portfolio Ledger Summary ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
    );

    f.render_widget(summary_widget, area);
}

pub fn format_chf(val: f64) -> String {
    let int_part = val.trunc().abs() as u64;
    let frac_part = (val.fract().abs() * 100.0).round() as u64;
    let sign = if val < -0.001 { "-" } else { "" };

    let s = int_part.to_string();
    let mut formatted_int = String::new();
    let len = s.len();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            formatted_int.push('\'');
        }
        formatted_int.push(ch);
    }
    format!("{}{}.{:02}", sign, formatted_int, frac_part % 100)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_chf() {
        assert_eq!(format_chf(0.0), "0.00");
        assert_eq!(format_chf(0.36), "0.36");
        assert_eq!(format_chf(12.81), "12.81");
        assert_eq!(format_chf(2370.34), "2'370.34");
        assert_eq!(format_chf(45655.52), "45'655.52");
        assert_eq!(format_chf(-1500.25), "-1'500.25");
    }

    #[test]
    fn test_format_balance_amount() {
        assert_eq!(format_balance_amount(0.0), "0.00");
        assert_eq!(format_balance_amount(7.654321), "7.6543");
        assert_eq!(format_balance_amount(0.00005432), "0.00005432");
    }

    #[test]
    fn test_render_with_session_and_stale_accounts() {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = AppState::new(vec![], vec![]);
        app.balances.push(crate::models::BalanceItem {
            account: "UH".to_string(),
            category: AccountCategory::Crypto,
            symbol: "BAT".to_string(),
            amount: 100.0,
            native_currency: "USD".to_string(),
            value_native: 20.0,
            value_chf: 16.0,
        });
        app.balances.push(crate::models::BalanceItem {
            account: "FP".to_string(),
            category: AccountCategory::Retirement,
            symbol: "finpension Equity 100".to_string(),
            amount: 3000.0,
            native_currency: "CHF".to_string(),
            value_native: 3000.0,
            value_chf: 3000.0,
        });
        app.mark_account_stale("FP");

        terminal.draw(|f| render(f, &app)).unwrap();
    }
}


