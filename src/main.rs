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
use models::{AccountCategory, AppState, BalanceItem};
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
    app_state.write().await.load_balance_cache();

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
                            state.tickers[pos].symbol = symbol.clone();
                            state.tickers[pos].price_str = price_str;
                            state.tickers[pos].price_num = price_num;
                            state.tickers[pos].last_success = Some(Instant::now());
                        }
                        if let Some(p) = price_num {
                            state.set_crypto_price(&symbol, p);
                        }
                        state.update_balance_values();
                    }
                }
                tokio::time::sleep(Duration::from_secs_f64(update_secs)).await;
            }
        });
    }

    // 3. Background worker: Live FX Rates
    {
        let app = Arc::clone(&app_state);
        let prov = Arc::clone(&providers);
        tokio::spawn(async move {
            loop {
                let rates = prov.fetch_fx_rates().await;
                {
                    let mut state = app.write().await;
                    state.fx_rates = rates;
                    state.update_balance_values();
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
    }

    // 4. Background worker: Accounts & Balances
    {
        let app = Arc::clone(&app_state);
        let prov = Arc::clone(&providers);
        let monero_addrs = config.get_monero_addresses();
        let uphold_tok = config.uphold_token.clone();
        let coinbase_key = config.coinbase_api_key.clone();
        let coinbase_secret = config.coinbase_api_secret.clone();
        let chia_path = config.chia_db_path.clone();
        let starling_tok = config.starling_token.clone();
        let kraken_key = config.kraken_api_key.clone();
        let kraken_secret = config.kraken_api_secret.clone();
        let mut ibkr_tok = config.ibkr_flex_token.clone();
        let mut ibkr_qid = config.ibkr_query_id.clone();
        let mut finpension_tok = config.finpension_token.clone();

        tokio::spawn(async move {
            let mut last_starling_poll: Option<Instant> = None;
            let mut starling_poll_interval = Duration::from_secs(60);
            let mut last_ibkr_poll: Option<Instant> = None;
            let mut ibkr_poll_interval = Duration::from_secs(600);
            let mut last_finpension_poll: Option<Instant> = None;
            let mut finpension_poll_interval = Duration::from_secs(600);

            loop {
                // Check Monero balances (MW)
                let mut mw_items = Vec::new();
                for addr in &monero_addrs {
                    if let Some(amt) = prov.fetch_monero_balance(addr).await
                        && amt > 0.0 {
                            let xmr_price = {
                                let cached = {
                                    let state = app.read().await;
                                    state.get_crypto_price("XMR")
                                };
                                if let Some(p) = cached {
                                    p
                                } else if let Some(p) = prov.fetch_asset_price_usd("XMR").await {
                                    let mut state = app.write().await;
                                    state.set_crypto_price("XMR", p);
                                    p
                                } else {
                                    0.0
                                }
                            };
                            let fx = {
                                let state = app.read().await;
                                state.fx_rates
                            };
                            let val_usd = amt * xmr_price;
                            let val_chf = fx.to_chf("USD", val_usd);
                            mw_items.push(BalanceItem {
                                account: "MW".to_string(),
                                category: AccountCategory::Crypto,
                                symbol: "XMR".to_string(),
                                amount: amt,
                                native_currency: "USD".to_string(),
                                value_native: val_usd,
                                value_chf: val_chf,
                            });
                        }
                }
                if !mw_items.is_empty() {
                    let mut state = app.write().await;
                    state.update_account_balances("MW", mw_items);
                }

                // Check Chia balance (CW)
                if let Some(amt) = prov.fetch_chia_balance(chia_path.as_deref())
                    && amt > 0.0 {
                        let xch_price = {
                            let cached = {
                                let state = app.read().await;
                                state.get_crypto_price("XCH")
                            };
                            if let Some(p) = cached {
                                p
                            } else if let Some(p) = prov.fetch_asset_price_usd("XCH").await {
                                let mut state = app.write().await;
                                state.set_crypto_price("XCH", p);
                                p
                            } else {
                                0.0
                            }
                        };
                        let fx = {
                            let state = app.read().await;
                            state.fx_rates
                        };
                        let val_usd = amt * xch_price;
                        let val_chf = fx.to_chf("USD", val_usd);
                        let cw_items = vec![BalanceItem {
                            account: "CW".to_string(),
                            category: AccountCategory::Crypto,
                            symbol: "XCH".to_string(),
                            amount: amt,
                            native_currency: "USD".to_string(),
                            value_native: val_usd,
                            value_chf: val_chf,
                        }];
                        let mut state = app.write().await;
                        state.update_account_balances("CW", cw_items);
                    }

                // Check Uphold balances (UH)
                let has_uh = uphold_tok.is_some() || {
                    let state = app.read().await;
                    state.balances.iter().any(|b| b.account == "UH")
                };
                if has_uh {
                    let tok_str = uphold_tok.as_deref().unwrap_or("");
                    if let Some(cards) = prov.fetch_uphold_cards(tok_str).await {
                        let mut uh_items = Vec::new();
                        for (curr, amt) in cards {
                            let (native_curr, val_native) = if curr == "USD" {
                                ("USD".to_string(), amt)
                            } else if curr == "EUR" {
                                ("EUR".to_string(), amt)
                            } else if curr == "GBP" {
                                ("GBP".to_string(), amt)
                            } else if curr == "CHF" {
                                ("CHF".to_string(), amt)
                            } else {
                                let price = {
                                    let cached = {
                                        let state = app.read().await;
                                        state.get_crypto_price(&curr)
                                    };
                                    if let Some(p) = cached {
                                        p
                                    } else if let Some(p) = prov.fetch_asset_price_usd(&curr).await {
                                        let mut state = app.write().await;
                                        state.set_crypto_price(&curr, p);
                                        p
                                    } else {
                                        0.0
                                    }
                                };
                                ("USD".to_string(), amt * price)
                            };
                            let val_chf = {
                                let state = app.read().await;
                                state.fx_rates.to_chf(&native_curr, val_native)
                            };
                            uh_items.push(BalanceItem {
                                account: "UH".to_string(),
                                category: AccountCategory::Crypto,
                                symbol: curr,
                                amount: amt,
                                native_currency: native_curr,
                                value_native: val_native,
                                value_chf: val_chf,
                            });
                        }
                        let mut state = app.write().await;
                        state.update_account_balances("UH", uh_items);
                    } else {
                        let mut state = app.write().await;
                        state.mark_account_stale("UH");
                    }
                }

                // Check Coinbase balances (CB)
                if let (Some(key), Some(secret)) = (&coinbase_key, &coinbase_secret) {
                    if let Some(cb_balances) = prov.fetch_coinbase_balances(key, secret).await {
                        let mut cb_items = Vec::new();
                        for (curr, amt, _) in cb_balances {
                            let (native_curr, val_native) = if curr == "USD" || curr == "USDC" {
                                ("USD".to_string(), amt)
                            } else {
                                let price = {
                                    let cached = {
                                        let state = app.read().await;
                                        state.get_crypto_price(&curr)
                                    };
                                    if let Some(p) = cached {
                                        p
                                    } else if let Some(p) = prov.fetch_asset_price_usd(&curr).await {
                                        let mut state = app.write().await;
                                        state.set_crypto_price(&curr, p);
                                        p
                                    } else {
                                        0.0
                                    }
                                };
                                ("USD".to_string(), amt * price)
                            };
                            let val_chf = {
                                let state = app.read().await;
                                state.fx_rates.to_chf(&native_curr, val_native)
                            };
                            cb_items.push(BalanceItem {
                                account: "CB".to_string(),
                                category: AccountCategory::Crypto,
                                symbol: curr,
                                amount: amt,
                                native_currency: native_curr,
                                value_native: val_native,
                                value_chf: val_chf,
                            });
                        }
                        let mut state = app.write().await;
                        state.update_account_balances("CB", cb_items);
                    } else {
                        let mut state = app.write().await;
                        state.mark_account_stale("CB");
                    }
                }

                // Check Starling Bank balances (ST)
                if let Some(ref tok) = starling_tok {
                    let should_poll = !matches!(last_starling_poll, Some(t) if t.elapsed() < starling_poll_interval);
                    if should_poll {
                        last_starling_poll = Some(Instant::now());
                        if let Some(st_balances) = prov.fetch_starling_balances(tok).await {
                            starling_poll_interval = Duration::from_secs(60);
                            let fx = {
                                let state = app.read().await;
                                state.fx_rates
                            };
                            let mut st_items = Vec::new();
                            for (curr, amt) in st_balances {
                                let val_chf = fx.to_chf(&curr, amt);
                                st_items.push(BalanceItem {
                                    account: "ST".to_string(),
                                    category: AccountCategory::Cash,
                                    symbol: curr.clone(),
                                    amount: amt,
                                    native_currency: curr,
                                    value_native: amt,
                                    value_chf: val_chf,
                                });
                            }
                            let mut state = app.write().await;
                            state.update_account_balances("ST", st_items);
                        } else {
                            // Backoff on rate limit or error
                            let mut state = app.write().await;
                            state.mark_account_stale("ST");
                            starling_poll_interval = Duration::from_secs(120);
                        }
                    }
                }

                // Check Kraken balances
                if let (Some(key), Some(secret)) = (&kraken_key, &kraken_secret) {
                    if let Some(kraken_balances) = prov.fetch_kraken_balances(key, secret).await {
                        let mut kraken_items = Vec::new();
                        for (curr, amt) in kraken_balances {
                            let (native_curr, val_native) = if curr == "USD" {
                                ("USD".to_string(), amt)
                            } else if curr == "CHF" {
                                ("CHF".to_string(), amt)
                            } else if curr == "EUR" {
                                ("EUR".to_string(), amt)
                            } else if curr == "GBP" {
                                ("GBP".to_string(), amt)
                            } else {
                                let price = {
                                    let cached = {
                                        let state = app.read().await;
                                        state.get_crypto_price(&curr)
                                    };
                                    if let Some(p) = cached {
                                        p
                                    } else if let Some(p) = prov.fetch_asset_price_usd(&curr).await {
                                        let mut state = app.write().await;
                                        state.set_crypto_price(&curr, p);
                                        p
                                    } else {
                                        0.0
                                    }
                                };
                                ("USD".to_string(), amt * price)
                            };
                            let val_chf = {
                                let state = app.read().await;
                                state.fx_rates.to_chf(&native_curr, val_native)
                            };
                            kraken_items.push(BalanceItem {
                                account: "Kraken".to_string(),
                                category: AccountCategory::Crypto,
                                symbol: curr,
                                amount: amt,
                                native_currency: native_curr,
                                value_native: val_native,
                                value_chf: val_chf,
                            });
                        }
                        let mut state = app.write().await;
                        state.update_account_balances("Kraken", kraken_items);
                    } else {
                        let mut state = app.write().await;
                        state.mark_account_stale("Kraken");
                    }
                }

                // Check Interactive Brokers balances (IB)
                if let Ok(fresh_cfg) = crate::config::Config::load_or_default::<&str>(None) {
                    let mut config_changed = false;
                    if fresh_cfg.ibkr_flex_token != ibkr_tok {
                        ibkr_tok = fresh_cfg.ibkr_flex_token;
                        config_changed = true;
                    }
                    if fresh_cfg.ibkr_query_id != ibkr_qid {
                        ibkr_qid = fresh_cfg.ibkr_query_id;
                        config_changed = true;
                    }
                    if config_changed {
                        ibkr_poll_interval = Duration::from_secs(600);
                        last_ibkr_poll = None;
                    }
                }

                if let (Some(tok), Some(qid)) = (&ibkr_tok, &ibkr_qid) {
                    let should_poll = !matches!(last_ibkr_poll, Some(t) if t.elapsed() < ibkr_poll_interval);
                    if should_poll {
                        last_ibkr_poll = Some(Instant::now());
                        if let Some(holdings) = prov.fetch_ibkr_holdings(tok, qid).await {
                            ibkr_poll_interval = Duration::from_secs(600);
                            let fx = {
                                let state = app.read().await;
                                state.fx_rates
                            };
                            let mut ib_items = Vec::new();
                            for h in holdings {
                                let val_chf = fx.to_chf(&h.currency, h.value_native);
                                ib_items.push(BalanceItem {
                                    account: "IB".to_string(),
                                    category: h.category,
                                    symbol: h.symbol,
                                    amount: h.amount,
                                    native_currency: h.currency,
                                    value_native: h.value_native,
                                    value_chf: val_chf,
                                });
                            }
                            let mut state = app.write().await;
                            state.update_account_balances("IB", ib_items);
                        } else {
                            // Exponential backoff on error: 15m -> 30m -> 1h -> 2h (capped)
                            let mut state = app.write().await;
                            state.mark_account_stale("IB");
                            if ibkr_poll_interval < Duration::from_secs(900) {
                                ibkr_poll_interval = Duration::from_secs(900);
                            } else {
                                ibkr_poll_interval = (ibkr_poll_interval * 2).min(Duration::from_secs(7200));
                            }
                        }
                    }
                }

                // Check Finpension 3a balances (FP)
                let should_poll = !matches!(last_finpension_poll, Some(t) if t.elapsed() < finpension_poll_interval);
                if should_poll {
                    last_finpension_poll = Some(Instant::now());
                    if let Some((portfolios, new_tok)) = prov.fetch_finpension_portfolios(finpension_tok.as_deref()).await {
                        if let Some(ref nt) = new_tok {
                            finpension_tok = Some(nt.clone());
                        }
                        finpension_poll_interval = Duration::from_secs(600);
                        let mut fp_items = Vec::new();
                        for (name, val_chf) in portfolios {
                            fp_items.push(BalanceItem {
                                account: "FP".to_string(),
                                category: AccountCategory::Retirement,
                                symbol: name,
                                amount: val_chf,
                                native_currency: "CHF".to_string(),
                                value_native: val_chf,
                                value_chf: val_chf,
                            });
                        }
                        let mut state = app.write().await;
                        state.update_account_balances("FP", fp_items);
                    } else {
                        // Backoff on rate limit, expired token, or no tab open
                        let mut state = app.write().await;
                        state.mark_account_stale("FP");
                        finpension_poll_interval = Duration::from_secs(120);
                    }
                }

                {
                    let mut state = app.write().await;
                    state.update_balance_values();
                }

                tokio::time::sleep(Duration::from_secs(30)).await;
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
        if event::poll(tick_rate)?
            && let Event::Key(key) = event::read()? {
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
