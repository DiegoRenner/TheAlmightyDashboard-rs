use crate::models::{AccountCategory, AppState, CustomCashModalMode, PrivacyMode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
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
    const TITLE: &str = " The Almighty Dashboard ";
    let mut header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            TITLE,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));

    if let Some(oldest) = app.most_outdated_field() {
        let field_color = oldest.status_color();
        header_block = header_block.title_bottom(Line::from(vec![
            Span::styled(" Most Outdated: ", Style::default().fg(Color::DarkGray)),
            Span::styled("● ", Style::default().fg(field_color)),
            Span::styled(oldest.name.clone(), Style::default().fg(field_color).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" · gathered {} ", oldest.time_display()),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    let (privacy_desc, privacy_style) = match app.privacy_mode {
        PrivacyMode::Normal => (" Privacy  ", Style::default().fg(Color::White)),
        PrivacyMode::HideAmounts => (" Privacy:No$  ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        PrivacyMode::HideAll => (" Privacy:No$+Qty  ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)),
    };

    // Notifications go on the border line so they stay visible on narrow terminals, trimmed to the
    // room the block title leaves so the two never overlap and eat each other's text
    if let Some(notif) = app.get_active_notification() {
        let room = usize::from(area.width).saturating_sub(TITLE.len() + 6);
        let text: String = notif.chars().take(room).collect();
        if !text.is_empty() {
            header_block = header_block.title_top(
                Line::from(Span::styled(
                    format!(" ★ {text} "),
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            );
        }
    }

    let spans = vec![
        Span::styled("[q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Quit "),
        Span::styled("[j/k]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Scroll "),
        Span::styled("[p]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(privacy_desc, privacy_style),
        Span::styled("[c]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Custom Cash "),
        Span::styled(
            format!(
                "| FX: USD {:.3} EUR {:.3} GBP {:.3} ",
                app.fx_rates.usd_to_chf, app.fx_rates.eur_to_chf, app.fx_rates.gbp_to_chf
            ),
            Style::default().fg(Color::Cyan),
        ),
    ];

    let header_widget = Paragraph::new(Line::from(spans)).block(header_block);
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

    if app.custom_cash_modal_open {
        render_custom_cash_modal(f, app, area);
    }
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
    if area.height < 13 {
        render_balances_table(f, app, area);
        return;
    }

    let sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(7)])
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
            let is_manual = app.is_custom_cash_account(&item.account);

            let acc_label = match (is_manual, is_session) {
                (true, _) => format!("{}~", item.account),
                (_, true) => format!("{}*", item.account),
                _ => item.account.clone(),
            };

            let acc_style = if is_manual {
                Style::default().fg(Color::White)
            } else if is_stale {
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
            } else if is_session {
                Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            };

            let amount_str = if app.privacy_mode.hides_quantities() {
                "******".to_string()
            } else {
                format_balance_amount(item.amount)
            };

            let val_str = if app.privacy_mode.hides_amounts() {
                "******".to_string()
            } else {
                format_chf(item.value_chf)
            };

            Row::new(vec![
                Cell::from(format!("{}", idx + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(acc_label).style(acc_style),
                Cell::from(item.category.to_string()).style(cat_style),
                Cell::from(item.symbol.clone()).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Cell::from(amount_str).style(Style::default().fg(Color::White)),
                Cell::from(val_str).style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ])
        })
        .collect();

    let mut table_title_spans = vec![
        Span::styled(" Holdings & Accounts ", Style::default().add_modifier(Modifier::BOLD)),
    ];
    match app.privacy_mode {
        PrivacyMode::Normal => {}
        PrivacyMode::HideAmounts => {
            table_title_spans.push(Span::styled("[🔒 Amounts Hidden] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        }
        PrivacyMode::HideAll => {
            table_title_spans.push(Span::styled("[🔒 Amounts & Qty Hidden] ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)));
        }
    }
    table_title_spans.extend(vec![
        Span::styled("(", Style::default().fg(Color::DarkGray)),
        Span::styled("● API", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("●* Session", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("● Stale", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("●~ Manual", Style::default().fg(Color::White)),
        Span::styled(") ", Style::default().fg(Color::DarkGray)),
    ]);

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
            .title(Line::from(table_title_spans))
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

    let hide_money = app.privacy_mode.hides_amounts();
    let format_val = |v: f64| -> String {
        if hide_money {
            "******".to_string()
        } else {
            format_chf(v)
        }
    };

    let is_fp_stale = app.is_account_stale("FP");
    let mut ret_spans = vec![
        Span::styled(
            " Retirement:    ",
            Style::default()
                .fg(if is_fp_stale { Color::LightRed } else { Color::Green })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("CHF {}", format_val(ret_val)),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ];
    if is_fp_stale {
        ret_spans.push(Span::styled(" (stale session)", Style::default().fg(Color::LightRed)));
    }

    let mut summary_lines = vec![
        Line::from(vec![
            Span::styled(" Stocks & Cash: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("CHF {} ", format_val(stocks_cash_val)), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("(Stocks: {} / Cash: {})", format_val(stocks_val), format_val(cash_val)), Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled(" Crypto:        ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(format!("CHF {}", format_val(crypto_val)), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(ret_spans),
        Line::from(vec![
            Span::styled(" Total (CHF):   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!("CHF {} ", format_val(total_net_worth)), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(format!("(~${} USD)", format_val(total_usd)), Style::default().fg(Color::DarkGray)),
        ]),
    ];

    if let Some(oldest) = app.most_outdated_field() {
        let field_color = oldest.status_color();
        summary_lines.push(Line::from(vec![
            Span::styled(" Oldest Data:   ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            Span::styled("● ", Style::default().fg(field_color)),
            Span::styled(format!("{} ", oldest.name), Style::default().fg(field_color).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("(gathered {})", oldest.time_display()),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let mut summary_title_spans = vec![
        Span::styled(" Portfolio Ledger Summary ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ];
    match app.privacy_mode {
        PrivacyMode::Normal => {}
        PrivacyMode::HideAmounts => {
            summary_title_spans.push(Span::styled("[🔒 Amounts Hidden] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        }
        PrivacyMode::HideAll => {
            summary_title_spans.push(Span::styled("[🔒 Amounts & Qty Hidden] ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)));
        }
    }

    let summary_widget = Paragraph::new(summary_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Line::from(summary_title_spans)),
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

/// Rows `text` occupies when greedily wrapped on word boundaries at `width`, matching how
/// `Paragraph` with `Wrap` lays it out.
fn wrapped_row_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut rows = 1;
    let mut col = 0;
    for word in text.split(' ') {
        let len = word.chars().count();
        if len > width {
            if col > 0 {
                rows += 1;
            }
            rows += (len - 1) / width;
            col = len % width;
        } else if col == 0 {
            col = len;
        } else if col + 1 + len <= width {
            col += 1 + len;
        } else {
            rows += 1;
            col = len;
        }
    }
    rows
}

fn render_custom_cash_modal(f: &mut Frame, app: &AppState, area: Rect) {
    let key = |k: &'static str| Span::styled(k, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
    let dim = Style::default().fg(Color::DarkGray);
    let input_lines = |what: &str, example: &str| -> Vec<Line> {
        let mut lines = vec![
            Line::from(vec![
                Span::raw(format!(" Enter {what} (e.g. ")),
                Span::styled(example.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("):"),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(" > ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(app.input_buffer.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("█", Style::default().fg(Color::Yellow)),
            ]),
            Line::from(""),
        ];
        lines.push(match &app.input_error {
            Some(err) => Line::from(Span::styled(
                format!(" ⚠ {err}"),
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
            )),
            None => Line::from(vec![key(" [Enter]"), Span::raw(" Save   "), key("[Esc]"), Span::raw(" Back")]),
        });
        lines
    };

    let (title, lines) = match app.custom_cash_modal_mode {
        CustomCashModalMode::List => {
            let hide = app.privacy_mode.hides_amounts();
            let mut lines: Vec<Line> = if app.custom_cash_items.is_empty() {
                vec![Line::from(Span::styled(" No custom cash fields yet.", dim))]
            } else {
                // ponytail: no scrolling inside the modal, rows past the modal height are clipped
                app.custom_cash_items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        let selected = i == app.custom_cash_selected_index;
                        let style = if selected {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let amount = if hide {
                            "******".to_string()
                        } else {
                            format!("{} {}", format_chf(item.amount), item.currency)
                        };
                        Line::from(vec![
                            Span::styled(if selected { " ▶ " } else { "   " }, style),
                            Span::styled(format!("{:<24}", item.name), style),
                            Span::styled(format!("{amount:>18}"), style),
                        ])
                    })
                    .collect()
            };
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                key(" [a]"),
                Span::raw(" Add  "),
                key("[e/Enter]"),
                Span::raw(" Edit  "),
                key("[d]"),
                Span::raw(" Delete  "),
                key("[j/k]"),
                Span::raw(" Select  "),
                key("[Esc]"),
                Span::raw(" Close"),
            ]));
            (" Custom Cash Fields ".to_string(), lines)
        }
        CustomCashModalMode::Add => (
            " Add Custom Cash Field ".to_string(),
            input_lines("name, amount & currency", "Chase 1234.50 GBP"),
        ),
        CustomCashModalMode::Edit => {
            let name = app
                .custom_cash_items
                .get(app.custom_cash_selected_index)
                .map(|i| i.name.as_str())
                .unwrap_or("?");
            (format!(" Edit: {name} "), input_lines("amount & currency", "1500 GBP"))
        }
    };

    let modal_width = 72.min(area.width.saturating_sub(4));
    // long inputs and error messages wrap instead of being clipped, so size the box for the rows the
    // wrap actually produces: a character count alone underestimates it and hides the last line
    let inner_width = usize::from(modal_width.saturating_sub(2).max(1));
    let wrapped_rows: usize = lines
        .iter()
        .map(|l| wrapped_row_count(&l.to_string(), inner_width))
        .sum();
    let modal_height = (wrapped_rows as u16 + 2).min(area.height.saturating_sub(2));
    let x = area.width.saturating_sub(modal_width) / 2;
    let y = area.height.saturating_sub(modal_height) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .title(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), modal_area);
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

    #[test]
    fn test_render_with_outdated_field() {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = AppState::new(vec![], vec![]);

        let now = std::time::SystemTime::now();
        let past = now - std::time::Duration::from_secs(863); // 14m 23s

        app.balances.push(crate::models::BalanceItem {
            account: "UBS".to_string(),
            category: AccountCategory::Cash,
            symbol: "CHF".to_string(),
            amount: 250.0,
            native_currency: "CHF".to_string(),
            value_native: 250.0,
            value_chf: 250.0,
        });
        app.account_last_gathered.insert("UBS".to_string(), past);

        // Render should succeed and include outdated field
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn test_render_privacy_mode_hide_amounts() {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = AppState::new(vec![], vec![]);
        app.balances.push(crate::models::BalanceItem {
            account: "UBS".to_string(),
            category: AccountCategory::Cash,
            symbol: "CHF".to_string(),
            amount: 250.0,
            native_currency: "CHF".to_string(),
            value_native: 250.0,
            value_chf: 250.0,
        });
        app.privacy_mode = PrivacyMode::HideAmounts;

        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("******"));
        assert!(content.contains("250.00"));
    }

    #[test]
    fn test_render_privacy_mode_hide_all() {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = AppState::new(vec![], vec![]);
        app.balances.push(crate::models::BalanceItem {
            account: "UBS".to_string(),
            category: AccountCategory::Cash,
            symbol: "CHF".to_string(),
            amount: 250.0,
            native_currency: "CHF".to_string(),
            value_native: 250.0,
            value_chf: 250.0,
        });
        app.privacy_mode = PrivacyMode::HideAll;

        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("******"));
        assert!(!content.contains("250.00"));
    }

    #[test]
    fn test_render_custom_cash_modal() {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = AppState::new(vec![], vec![]);
        app.custom_cash_items.push(crate::config::CustomCashItem {
            name: "Chase".to_string(),
            amount: 1234.50,
            currency: "GBP".to_string(),
        });
        app.custom_cash_modal_open = true;

        let text = |t: &ratatui::Terminal<ratatui::backend::TestBackend>| -> String {
            t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
        };

        terminal.draw(|f| render(f, &app)).unwrap();
        let content = text(&terminal);
        assert!(content.contains("Custom Cash Fields"));
        assert!(content.contains("Chase"));
        assert!(content.contains("1'234.50 GBP"));

        app.custom_cash_modal_mode = CustomCashModalMode::Add;
        app.input_buffer = "Safe 250 CHF".to_string();
        terminal.draw(|f| render(f, &app)).unwrap();
        let content = text(&terminal);
        assert!(content.contains("Add Custom Cash Field"));
        assert!(content.contains("Safe 250 CHF"));

        app.custom_cash_modal_mode = CustomCashModalMode::Edit;
        app.input_error = Some("bad input".to_string());
        terminal.draw(|f| render(f, &app)).unwrap();
        let content = text(&terminal);
        assert!(content.contains("Edit: Chase"));
        assert!(content.contains("bad input"));

        // input longer than the modal wraps onto a second row instead of being clipped
        app.custom_cash_modal_mode = CustomCashModalMode::Add;
        app.input_error = None;
        app.input_buffer = "A very long account description that keeps going well past the box edge 1234.50 GBP".to_string();
        terminal.draw(|f| render(f, &app)).unwrap();
        let content = text(&terminal);
        assert!(content.contains("1234.50 GBP"));

        // a wrapped input must not push the error line out of the box on a narrow terminal
        let mut narrow = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 30)).unwrap();
        app.input_buffer = "Emergency-Fund-Under-The-Mattress 1234.50 XYZ".to_string();
        app.input_error = Some("Unsupported currency 'XYZ' (use CHF/USD/EUR/GBP/AUD)".to_string());
        narrow.draw(|f| render(f, &app)).unwrap();
        let content = text(&narrow);
        assert!(content.contains("Unsupported currency"), "error line was clipped");
    }

    #[test]
    fn test_wrapped_row_count() {
        assert_eq!(wrapped_row_count("", 10), 1);
        assert_eq!(wrapped_row_count("hello world", 11), 1);
        assert_eq!(wrapped_row_count("hello world", 10), 2);
        // words that do not pack evenly need more rows than a character count suggests
        assert_eq!(wrapped_row_count("aaaaaa bbbbbb cccccc", 10), 3);
        // a word longer than the width is broken across rows
        assert_eq!(wrapped_row_count("aaaaaaaaaaaaaaaaaaaaaa", 10), 3);
    }
}


