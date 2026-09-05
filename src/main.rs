mod config;
mod models;
mod providers;
mod ui;

use clap::Parser;
use config::Config;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use models::{AppState, BalanceItem};
use providers::Providers;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::{
    io::{self, stdout},
    panic,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Parser, Debug)]
#[command(author, version, about = "The Almighty Dashboard - Rust Edition")]
struct Args {
    /// Path to config JSON file
    #[arg(value_name = "CONFIG")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = Config::load_or_default(args.config.as_ref())?;

    let update_secs = config.update_frequency_secs.max(1.0);
    let app_state = Arc::new(RwLock::new(AppState::new(
        config.coinmarketcap_urls.clone(),
        config.marketwatch_urls.clone(),
    )));

    // Setup terminal with panic hook for safe recovery
    setup_panic_hook();
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let providers = Arc::new(Providers::new());

    // 1. Background worker: Stocks
    {
        let app = Arc::clone(&app_state);
        let prov = Arc::clone(&providers);
        let stock_targets = config.marketwatch_urls.clone();
        tokio::spawn(async move {
            loop {
                for (idx, target) in stock_targets.iter().enumerate() {
                    let (symbol, price_str, price_num) = prov.fetch_stock(target).await;
                    {
                        let mut state = app.write().await;
                        if idx < state.tickers.len() {
                            state.tickers[idx].symbol = symbol;
                            state.tickers[idx].price_str = price_str;
                            state.tickers[idx].price_num = price_num;
                            state.tickers[idx].last_success = Some(Instant::now());
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs_f64(update_secs)).await;
            }
        });
    }

    // 2. Background worker: Cryptocurrencies
    {
        let app = Arc::clone(&app_state);
        let prov = Arc::clone(&providers);
        let crypto_targets = config.coinmarketcap_urls.clone();
        let offset = config.marketwatch_urls.len();
        tokio::spawn(async move {
            loop {
                for (idx, target) in crypto_targets.iter().enumerate() {
                    let (symbol, price_str, price_num) = prov.fetch_crypto(target).await;
                    {
                        let mut state = app.write().await;
                        let pos = offset + idx;
                        if pos < state.tickers.len() {
                            state.tickers[pos].symbol = symbol;
                            state.tickers[pos].price_str = price_str;
                            state.tickers[pos].price_num = price_num;
                            state.tickers[pos].last_success = Some(Instant::now());
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs_f64(update_secs)).await;
            }
        });
    }

    // 3. Background worker: Accounts & Balances
    {
        let app = Arc::clone(&app_state);
        let prov = Arc::clone(&providers);
        let monero_addrs = config.get_monero_addresses();
        let uphold_tok = config.uphold_token.clone();
        let coinbase_key = config.coinbase_api_key.clone();
        let coinbase_secret = config.coinbase_api_secret.clone();

        tokio::spawn(async move {
            loop {
                let mut new_balances = Vec::new();

                // Check Monero balances
                for addr in &monero_addrs {
                    if let Some(amt) = prov.fetch_monero_balance(addr).await {
                        if amt > 0.0 {
                            let xmr_price = {
                                let state = app.read().await;
                                state.get_crypto_price("XMR").unwrap_or(0.0)
                            };
                            new_balances.push(BalanceItem {
                                symbol: "XMR".to_string(),
                                amount: amt,
                                value_usd: amt * xmr_price,
                            });
                        }
                    }
                }

                // Check Uphold balances
                if let Some(ref tok) = uphold_tok {
                    let cards = prov.fetch_uphold_cards(tok).await;
                    let state = app.read().await;
                    for (curr, amt) in cards {
                        let val = if curr == "USD" {
                            amt
                        } else {
                            let price = state.get_crypto_price(&curr).unwrap_or(0.0);
                            amt * price
                        };
                        new_balances.push(BalanceItem {
                            symbol: curr,
                            amount: amt,
                            value_usd: val,
                        });
                    }
                }

                // Check Coinbase balances
                if let (Some(key), Some(secret)) = (&coinbase_key, &coinbase_secret) {
                    let cb_balances = prov.fetch_coinbase_balances(key, secret).await;
                    for (curr, amt, val) in cb_balances {
                        new_balances.push(BalanceItem {
                            symbol: curr,
                            amount: amt,
                            value_usd: val,
                        });
                    }
                }

                {
                    let mut state = app.write().await;
                    state.balances = new_balances;
                }

                tokio::time::sleep(Duration::from_secs_f64(update_secs)).await;
            }
        });
    }

    // 4. Main UI and Event loop
    let tick_rate = Duration::from_millis(100);
    loop {
        // Update elapsed timers
        {
            let mut state = app_state.write().await;
            for t in &mut state.tickers {
                t.update_elapsed();
            }
            if state.should_quit {
                break;
            }
        }

        // Render UI
        {
            let state = app_state.read().await;
            terminal.draw(|f| ui::render(f, &state))?;
        }

        // Handle keyboard input (non-blocking poll)
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                let mut state = app_state.write().await;
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        state.should_quit = true;
                        break;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.scroll_down();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.scroll_up();
                    }
                    _ => {}
                }
            }
        }
    }

    // Restore terminal cleanly
    teardown_terminal()?;
    Ok(())
}

fn setup_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = teardown_terminal();
        original_hook(panic_info);
    }));
}

fn teardown_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen, Show)?;
    Ok(())
}
