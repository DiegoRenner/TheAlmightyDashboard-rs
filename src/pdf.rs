use crate::models::{AccountCategory, AppState};
use crate::ui::{format_chf, format_grouped};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Whether pdflatex can typeset `c` with this document's preamble (inputenc utf8 + fontenc T1 +
/// lmodern). The set was probed against pdflatex itself: a codepoint threshold gets it wrong both
/// ways, passing Latin Extended-B characters that abort the run and rejecting punctuation that works.
fn latex_typesettable(c: char) -> bool {
    match c {
        ' '..='~' => true,
        '\u{A1}'..='\u{FF}' => c != '\u{AD}',
        // Latin Extended-A, minus the codepoints T1 has no glyph for
        '\u{100}'..='\u{17E}' => !matches!(
            c,
            '\u{126}' | '\u{127}' | '\u{138}' | '\u{13F}' | '\u{140}' | '\u{149}' | '\u{166}' | '\u{167}'
        ),
        '\u{2013}' | '\u{2014}' | '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}' => true,
        '\u{2026}' | '\u{20AC}' | '\u{2122}' => true,
        _ => false,
    }
}

pub fn escape_latex(s: &str) -> String {
    let mut res = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => res.push_str("\\&"),
            '%' => res.push_str("\\%"),
            '$' => res.push_str("\\$"),
            '#' => res.push_str("\\#"),
            '_' => res.push_str("\\_"),
            '{' => res.push_str("\\{"),
            '}' => res.push_str("\\}"),
            '~' => res.push_str("\\textasciitilde{}"),
            '^' => res.push_str("\\textasciicircum{}"),
            '\\' => res.push_str("\\textbackslash{}"),
            // user-typed names: anything this preamble cannot typeset would abort pdflatex
            c if c.is_control() => res.push(' '),
            c if latex_typesettable(c) => res.push(c),
            _ => res.push('?'),
        }
    }
    res
}

pub fn format_latex_amount(amount: f64) -> String {
    if amount == 0.0 {
        "0.00".to_string()
    } else if amount.abs() >= 1000.0 {
        format_grouped(amount, 4)
    } else if amount.abs() >= 0.0001 {
        format!("{:.4}", amount)
    } else {
        format!("{:.8}", amount)
    }
}

#[derive(Debug, Clone)]
pub struct TimeframeInfo {
    pub oldest_time: std::time::SystemTime,
    pub oldest_source: String,
    pub latest_time: std::time::SystemTime,
    pub latest_source: String,
    pub span_secs: u64,
}

pub fn get_data_collection_timeframe(app: &AppState) -> Option<TimeframeInfo> {
    let mut entries: Vec<(String, std::time::SystemTime)> = Vec::new();

    // Accounts for holdings present in balances
    for b in &app.balances {
        if let Some(t) = app.account_last_gathered.get(&b.account) {
            entries.push((b.account.clone(), *t));
        }
    }

    // Tickers
    for t in &app.tickers {
        if let Some(g) = t.last_gathered {
            entries.push((t.symbol.clone(), g));
        }
    }

    if entries.is_empty() {
        return None;
    }

    let oldest = entries.iter().min_by_key(|(_, t)| *t)?;
    let latest = entries.iter().max_by_key(|(_, t)| *t)?;

    let span_secs = latest.1.duration_since(oldest.1).map(|d| d.as_secs()).unwrap_or(0);

    Some(TimeframeInfo {
        oldest_time: oldest.1,
        oldest_source: oldest.0.clone(),
        latest_time: latest.1,
        latest_source: latest.0.clone(),
        span_secs,
    })
}

pub fn generate_latex(app: &AppState) -> String {
    let now = chrono::Local::now();
    let now_str = now.format("%d.%m.%Y %H:%M:%S").to_string();

    let total_chf = app.total_net_worth_chf();
    let total_usd = app.total_balance_usd();
    let crypto_chf = app.crypto_total_chf();
    let stocks_chf = app.stocks_total_chf();
    let cash_chf = app.cash_total_chf();
    let retirement_chf = app.retirement_total_chf();

    // + 0.0 normalises the -0.0 that summing an empty category yields, which would print as "-0.0%"
    let pct = |v: f64| if total_chf > 0.0 { (v / total_chf) * 100.0 + 0.0 } else { 0.0 };
    let crypto_pct = pct(crypto_chf);
    let stocks_pct = pct(stocks_chf);
    let cash_pct = pct(cash_chf);
    let retirement_pct = pct(retirement_chf);

    let usd_rate = app.fx_rates.usd_to_chf;
    let crypto_usd = if usd_rate > 0.0 { crypto_chf / usd_rate } else { 0.0 };
    let stocks_usd = if usd_rate > 0.0 { stocks_chf / usd_rate } else { 0.0 };
    let cash_usd = if usd_rate > 0.0 { cash_chf / usd_rate } else { 0.0 };
    let retirement_usd = if usd_rate > 0.0 { retirement_chf / usd_rate } else { 0.0 };

    let mut tex = String::new();

    // LaTeX Document Header
    tex.push_str(r#"\documentclass[10pt,a4paper]{article}
\usepackage[a4paper, margin=1.4cm, top=1.8cm, bottom=2.0cm]{geometry}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage{lmodern}
\usepackage{microtype}
\usepackage{booktabs}
\usepackage{tabularx}
\usepackage{xltabular}
\usepackage{xcolor}
\usepackage{fancyhdr}
\usepackage{lastpage}

\definecolor{primary}{RGB}{30, 41, 59}
\definecolor{accent}{RGB}{14, 116, 144}
\definecolor{headerbg}{RGB}{241, 245, 249}
\definecolor{crypto}{RGB}{124, 58, 237}
\definecolor{stocks}{RGB}{37, 99, 235}
\definecolor{cash}{RGB}{13, 148, 136}
\definecolor{retirement}{RGB}{22, 163, 74}
\definecolor{stale}{RGB}{220, 38, 38}
\definecolor{live}{RGB}{22, 101, 52}

\pagestyle{fancy}
\fancyhf{}
\rfoot{\footnotesize\color{gray} Page \thepage\ of \pageref{LastPage}}
\lfoot{\footnotesize\color{gray} The Almighty Dashboard --- Confidential Portfolio Statement}
\renewcommand{\headrulewidth}{0pt}
\renewcommand{\footrulewidth}{0.4pt}

\setlength{\LTleft}{0pt}
\setlength{\LTright}{0pt}

\begin{document}
"#);

    let timeframe_banner = if let Some(tf) = get_data_collection_timeframe(app) {
        let dt_oldest: chrono::DateTime<chrono::Local> = tf.oldest_time.into();
        let dt_latest: chrono::DateTime<chrono::Local> = tf.latest_time.into();
        let oldest_str = dt_oldest.format("%d.%m.%Y %H:%M:%S").to_string();
        let latest_str = dt_latest.format("%d.%m.%Y %H:%M:%S").to_string();

        let span_str = if tf.span_secs < 60 {
            format!("{}s", tf.span_secs)
        } else if tf.span_secs < 3600 {
            format!("{}m {:02}s", tf.span_secs / 60, tf.span_secs % 60)
        } else if tf.span_secs < 86400 {
            format!("{}h {:02}m", tf.span_secs / 3600, (tf.span_secs % 3600) / 60)
        } else {
            format!("{}d {:02}h", tf.span_secs / 86400, (tf.span_secs % 86400) / 3600)
        };

        format!(
            r#"\noindent
\colorbox{{headerbg}}{{%
\parbox{{\dimexpr\textwidth-2\fboxsep\relax}}{{%
\small
\textbf{{Data Collection Timeframe:}} From \textbf{{{}}} (Oldest: {}) to \textbf{{{}}} (Latest: {}) \hfill \textbf{{Timespan:}} {}
}}%
}}"#,
            oldest_str,
            escape_latex(&tf.oldest_source),
            latest_str,
            escape_latex(&tf.latest_source),
            span_str
        )
    } else {
        format!(
            r#"\noindent
\colorbox{{headerbg}}{{%
\parbox{{\dimexpr\textwidth-2\fboxsep\relax}}{{%
\small
\textbf{{Data Collection Timeframe:}} Live Snapshot (\textbf{{{}}})
}}%
}}"#,
            now_str
        )
    };

    // Title banner
    tex.push_str(&format!(
        r#"\noindent
\begin{{tabular*}}{{\textwidth}}{{@{{\extracolsep{{\fill}}}} l r @{{}}}}
{{\LARGE\bfseries\color{{primary}} The Almighty Dashboard}} & \textbf{{Generated:}} {} \\
{{\large\bfseries\color{{accent}} Portfolio Holdings Statement}} & \textbf{{Base Currency:}} CHF (Swiss Franc) \\
\end{{tabular*}}

\vspace{{0.4em}}
{}

\vspace{{0.6em}}
\hrule height 1pt
\vspace{{0.8em}}
"#,
        now_str,
        timeframe_banner
    ));

    // Executive Summary & FX Rates
    tex.push_str(&format!(
        r#"\noindent\textbf{{\large Executive Summary \& Asset Allocation}}
\vspace{{0.3em}}

\noindent
\begin{{tabular*}}{{\textwidth}}{{@{{\extracolsep{{\fill}}}} l r r r @{{}}}}
\toprule
\textbf{{Asset Class}} & \textbf{{Valuation (CHF)}} & \textbf{{Valuation (USD)}} & \textbf{{Portfolio Weight}} \\
\midrule
\textcolor{{crypto}}{{\textbf{{Crypto}}}} & {} & {} & {:.1}\% \\
\textcolor{{stocks}}{{\textbf{{Stocks}}}} & {} & {} & {:.1}\% \\
\textcolor{{cash}}{{\textbf{{Cash}}}} & {} & {} & {:.1}\% \\
\textcolor{{retirement}}{{\textbf{{Retirement}}}} & {} & {} & {:.1}\% \\
\midrule
\textbf{{Total Net Worth}} & \textbf{{{}}} & \textbf{{{}}} & \textbf{{100.0\%}} \\
\bottomrule
\end{{tabular*}}

\vspace{{0.5em}}
\noindent
\footnotesize\textbf{{Market FX Rates at Snapshot:}} USD/CHF {:.3} \quad$\cdot$\quad EUR/CHF {:.3} \quad$\cdot$\quad GBP/CHF {:.3} \quad$\cdot$\quad AUD/CHF {:.3}
\normalsize

\vspace{{1.2em}}
"#,
        format_chf(crypto_chf), format_chf(crypto_usd), crypto_pct,
        format_chf(stocks_chf), format_chf(stocks_usd), stocks_pct,
        format_chf(cash_chf), format_chf(cash_usd), cash_pct,
        format_chf(retirement_chf), format_chf(retirement_usd), retirement_pct,
        format_chf(total_chf), format_chf(total_usd),
        app.fx_rates.usd_to_chf, app.fx_rates.eur_to_chf, app.fx_rates.gbp_to_chf, app.fx_rates.aud_to_chf
    ));

    // Holdings Table
    tex.push_str(r#"\noindent\textbf{\large Detailed Holdings Inventory}
\vspace{0.3em}

{\small
\begin{xltabular}{\textwidth}{@{} r l l >{\raggedright\arraybackslash}X r r r l @{}}
\toprule
\textbf{\#} & \textbf{Account} & \textbf{Category} & \textbf{Asset} & \textbf{Quantity} & \textbf{Native Value} & \textbf{Val (CHF)} & \textbf{Gathered} \\
\midrule
\endfirsthead
\toprule
\textbf{\#} & \textbf{Account} & \textbf{Category} & \textbf{Asset} & \textbf{Quantity} & \textbf{Native Value} & \textbf{Val (CHF)} & \textbf{Gathered} \\
\midrule
\endhead
\midrule
\multicolumn{8}{r}{\footnotesize\color{gray}\textit{Continued on next page\dots}} \\
\endfoot
\bottomrule
\endlastfoot
"#);

    if app.balances.is_empty() {
        tex.push_str(r#"\multicolumn{8}{c}{\textit{No holdings loaded}} \\"#);
        tex.push('\n');
    } else {
        for (idx, item) in app.balances.iter().enumerate() {
            let cat_color = match item.category {
                AccountCategory::Crypto => "crypto",
                AccountCategory::Stocks => "stocks",
                AccountCategory::Cash => "cash",
                AccountCategory::Retirement => "retirement",
            };

            let is_stale = app.is_account_stale(&item.account);
            let sync_info = if app.is_custom_cash_account(&item.account) {
                format!("{} (Manual)", now.format("%d.%m.%Y %H:%M"))
            } else if let Some(time) = app.account_last_gathered.get(&item.account) {
                let dt: chrono::DateTime<chrono::Local> = (*time).into();
                let time_display = dt.format("%d.%m.%Y %H:%M:%S").to_string();
                let status_label = if is_stale {
                    r#"\textcolor{stale}{\textbf{Stale}}"#
                } else {
                    r#"\textcolor{live}{Live}"#
                };
                format!("{} ({})", time_display, status_label)
            } else {
                "Not recorded".to_string()
            };

            let native_str = format!("{} {}", format_chf(item.value_native), escape_latex(&item.native_currency));

            tex.push_str(&format!(
                "{} & {} & \\textcolor{{{}}}{{\\textbf{{{}}}}} & {} & {} & {} & {} & {} \\\\\n",
                idx + 1,
                escape_latex(&item.account),
                cat_color,
                escape_latex(&item.category.to_string()),
                escape_latex(&item.symbol),
                format_latex_amount(item.amount),
                native_str,
                format_chf(item.value_chf),
                sync_info
            ));
        }
    }

    tex.push_str(r#"\end{xltabular}
}
"#);

    // Tracked Market Quotes Section (if tickers exist)
    if !app.tickers.is_empty() {
        tex.push_str(r#"
\vspace{1.0em}
\noindent\textbf{\large Reference Market Quotes Snapshot}
\vspace{0.3em}

{\small
\begin{xltabular}{\textwidth}{@{} l l r r >{\raggedright\arraybackslash}X @{}}
\toprule
\textbf{Symbol} & \textbf{Type} & \textbf{Price [\$]} & \textbf{Latency} & \textbf{Gathered Timestamp} \\
\midrule
\endfirsthead
\toprule
\textbf{Symbol} & \textbf{Type} & \textbf{Price [\$]} & \textbf{Latency} & \textbf{Gathered Timestamp} \\
\midrule
\endhead
\midrule
\multicolumn{5}{r}{\footnotesize\color{gray}\textit{Continued on next page\dots}} \\
\endfoot
\bottomrule
\endlastfoot
"#);
        for t in &app.tickers {
            let ty = if t.is_crypto { "Crypto" } else { "Stock" };
            let time_str = t
                .last_gathered
                .map(|g| {
                    let dt: chrono::DateTime<chrono::Local> = g.into();
                    dt.format("%d.%m.%Y %H:%M:%S").to_string()
                })
                .unwrap_or_else(|| "unloaded".to_string());

            let delay_str = if t.delay_ms > 0 {
                format!("{} ms", t.delay_ms)
            } else {
                "-".to_string()
            };

            tex.push_str(&format!(
                "{} & {} & {} & {} & {} \\\\\n",
                escape_latex(&t.symbol),
                ty,
                escape_latex(&t.price_str),
                delay_str,
                time_str
            ));
        }

        tex.push_str(r#"\end{xltabular}
}
"#);
    }

    tex.push_str(r#"
\end{document}
"#);

    tex
}

/// Exports the current holdings to a timestamped PDF in the working directory.
pub fn export_pdf(app: &AppState) -> Result<String, String> {
    compile_pdf(&generate_latex(app), None)
}

/// Runs pdflatex on `tex` in a temp dir and copies the result to `output_path` (default: timestamped
/// name in the cwd). Blocking: call it off the async event loop.
pub fn compile_pdf(tex: &str, output_path: Option<&Path>) -> Result<String, String> {
    let default_name = format!(
        "portfolio_statement_{}.pdf",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let pdf_path = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&default_name));

    let temp_dir = std::env::temp_dir().join(format!(
        "dashboard_pdf_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temporary directory: {e}"))?;
    let result = compile_in(&temp_dir, tex, &pdf_path);
    let _ = std::fs::remove_dir_all(&temp_dir);
    result.map(|()| pdf_path.display().to_string())
}

fn compile_in(temp_dir: &Path, tex: &str, pdf_path: &Path) -> Result<(), String> {
    let temp_tex = temp_dir.join("statement.tex");
    std::fs::write(&temp_tex, tex)
        .map_err(|e| format!("Failed to write temporary LaTeX file: {e}"))?;

    // Run pdflatex twice to resolve longtable column widths and LastPage cross-references
    for run in 1..=2 {
        let output = Command::new("pdflatex")
            .arg("-interaction=nonstopmode")
            .arg("-output-directory")
            .arg(temp_dir)
            .arg(&temp_tex)
            .output()
            .map_err(|e| format!("Failed to execute pdflatex: {e}. Ensure pdflatex is installed."))?;

        if !output.status.success() {
            let log_content = std::fs::read_to_string(temp_dir.join("statement.log")).unwrap_or_default();
            let last_lines: Vec<&str> = log_content.lines().rev().take(15).collect();
            return Err(format!(
                "pdflatex compilation failed (run {run}):\n{}",
                last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
            ));
        }
    }

    let generated_pdf = temp_dir.join("statement.pdf");
    if !generated_pdf.exists() {
        return Err("pdflatex finished but no statement.pdf was created".to_string());
    }
    std::fs::copy(&generated_pdf, pdf_path)
        .map(|_| ())
        .map_err(|e| format!("Failed to save output PDF to {}: {e}", pdf_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BalanceItem, FxRates, TickerItem};

    #[test]
    fn test_escape_latex() {
        assert_eq!(escape_latex("Stocks & Bonds"), "Stocks \\& Bonds");
        assert_eq!(escape_latex("100% Cash"), "100\\% Cash");
        assert_eq!(escape_latex("$USD_Balance#1"), "\\$USD\\_Balance\\#1");
        assert_eq!(escape_latex("{special}"), "\\{special\\}");
    }

    #[test]
    fn test_format_latex_amount() {
        assert_eq!(format_latex_amount(0.0), "0.00");
        assert_eq!(format_latex_amount(12500.5), "12'500.5000");
        assert_eq!(format_latex_amount(1234.99996), "1'235.0000");
        assert_eq!(format_latex_amount(7.654321), "7.6543");
        assert_eq!(format_latex_amount(0.00005432), "0.00005432");
    }

    #[test]
    fn test_escape_latex_degrades_unicode() {
        // typesettable: ASCII, Latin-1, Latin Extended-A, common punctuation and the euro sign
        assert_eq!(escape_latex("Café"), "Café");
        assert_eq!(escape_latex("Łódź"), "Łódź");
        assert_eq!(escape_latex("Mum's €5 – envelope"), "Mum's €5 – envelope");
        // not typesettable: these abort pdflatex, so they degrade instead
        assert_eq!(escape_latex("Piggy 🐷\tbank"), "Piggy ? bank");
        assert_eq!(escape_latex("Bitcoin ₿"), "Bitcoin ?");
        assert_eq!(escape_latex("Ħal Ưu Ŧ"), "?al ?u ?");
    }

    #[test]
    fn test_generate_latex_contains_holdings_and_timestamps() {
        let mut state = AppState::new(vec![], vec![]);
        state.fx_rates = FxRates::default();
        state.update_account_balances(
            "Swissquote",
            vec![BalanceItem {
                account: "Swissquote".to_string(),
                category: AccountCategory::Stocks,
                symbol: "VT".to_string(),
                amount: 100.0,
                native_currency: "USD".to_string(),
                value_native: 12000.0,
                value_chf: 9720.0,
            }],
        );
        state
            .add_custom_cash_item(crate::config::CustomCashItem {
                name: "Chase".to_string(),
                amount: 1500.0,
                currency: "GBP".to_string(),
            })
            .unwrap();

        let tex = generate_latex(&state);
        assert!(tex.contains("Swissquote"));
        assert!(tex.contains("VT"));
        let chase_row = tex.lines().find(|l| l.contains("Chase")).expect("Chase row");
        assert!(chase_row.contains("(Manual)"), "{chase_row}");
        assert!(!chase_row.contains("Live") && !chase_row.contains("Stale"), "{chase_row}");
        assert!(tex.contains("Portfolio Holdings Statement"));
        assert!(tex.contains("Detailed Holdings Inventory"));
        assert!(tex.contains("Executive Summary"));
    }

    #[test]
    fn test_export_pdf_live() {
        // pdflatex is a runtime dependency, not a build one: skip instead of failing where it is absent
        if Command::new("pdflatex").arg("--version").output().is_err() {
            eprintln!("skipping test_export_pdf_live: pdflatex not installed");
            return;
        }

        let mut state = AppState::new(vec![], vec![]);
        state.tickers.push(TickerItem::new("BTC-USD", true));
        state
            .add_custom_cash_item(crate::config::CustomCashItem {
                name: "Cash/Misc".to_string(),
                amount: 2500.0,
                currency: "CHF".to_string(),
            })
            .unwrap();

        let target_pdf = std::env::temp_dir().join(format!("dashboard_test_{}.pdf", std::process::id()));

        let result = compile_pdf(&generate_latex(&state), Some(&target_pdf));
        // collect the facts and clean up before asserting, so a failure leaves nothing behind
        let size = std::fs::metadata(&target_pdf).map(|m| m.len()).unwrap_or(0);
        let _ = std::fs::remove_file(&target_pdf);

        assert!(result.is_ok(), "PDF export failed: {:?}", result.err());
        assert!(size > 1000, "PDF is suspiciously small: {size} bytes");
    }

    #[test]
    fn test_data_collection_timeframe() {
        let mut state = AppState::new(vec![], vec![]);
        let now = std::time::SystemTime::now();
        let one_hour_ago = now - std::time::Duration::from_secs(3600);

        state.update_account_balances_with_time(
            "IB",
            vec![BalanceItem {
                account: "IB".to_string(),
                category: AccountCategory::Stocks,
                symbol: "AAPL".to_string(),
                amount: 10.0,
                native_currency: "USD".to_string(),
                value_native: 1500.0,
                value_chf: 1215.0,
            }],
            one_hour_ago,
        );

        state.update_account_balances_with_time(
            "SQ",
            vec![BalanceItem {
                account: "SQ".to_string(),
                category: AccountCategory::Stocks,
                symbol: "VT".to_string(),
                amount: 50.0,
                native_currency: "USD".to_string(),
                value_native: 5000.0,
                value_chf: 4050.0,
            }],
            now,
        );

        // a manual field must never become the oldest/latest data source
        state
            .add_custom_cash_item(crate::config::CustomCashItem {
                name: "Chase".to_string(),
                amount: 1.0,
                currency: "GBP".to_string(),
            })
            .unwrap();

        let tf = get_data_collection_timeframe(&state).unwrap();
        assert_eq!(tf.oldest_source, "IB");
        assert_eq!(tf.latest_source, "SQ");
        assert!(tf.span_secs >= 3600);

        let tex = generate_latex(&state);
        assert!(tex.contains("Data Collection Timeframe"));
        assert!(tex.contains("Oldest: IB"));
        assert!(tex.contains("Latest: SQ"));
    }
}
