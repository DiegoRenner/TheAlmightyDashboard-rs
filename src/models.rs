use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TickerItem {
    pub symbol: String,
    pub price_str: String,
    pub price_num: Option<f64>,
    pub last_success: Option<Instant>,
    pub delay_ms: u64,
    pub is_crypto: bool,
}

impl TickerItem {
    pub fn new(target: &str, is_crypto: bool) -> Self {
        let initial_sym = target
            .trim_end_matches('/')
            .split('/')
            .next_back()
            .unwrap_or("unloaded")
            .to_uppercase();

        Self {
            symbol: initial_sym,
            price_str: "unloaded".to_string(),
            price_num: None,
            last_success: None,
            delay_ms: 0,
            is_crypto,
        }
    }

    pub fn update_elapsed(&mut self) {
        if let Some(instant) = self.last_success {
            self.delay_ms = instant.elapsed().as_millis() as u64;
        }
    }
}

#[derive(Debug, Clone)]
pub struct BalanceItem {
    pub symbol: String,
    pub amount: f64,
    pub value_usd: f64,
}

#[derive(Debug)]
pub struct AppState {
    pub tickers: Vec<TickerItem>,
    pub balances: Vec<BalanceItem>,
    pub scroll_offset: usize,
    pub should_quit: bool,
}

impl AppState {
    pub fn new(crypto_targets: Vec<String>, stock_targets: Vec<String>) -> Self {
        let mut tickers = Vec::new();

        for s in &stock_targets {
            tickers.push(TickerItem::new(s, false));
        }
        for c in &crypto_targets {
            tickers.push(TickerItem::new(c, true));
        }

        Self {
            tickers,
            balances: Vec::new(),
            scroll_offset: 0,
            should_quit: false,
        }
    }

    pub fn total_balance_usd(&self) -> f64 {
        self.balances.iter().map(|b| b.value_usd).sum()
    }

    pub fn scroll_down(&mut self) {
        let max_items = self.tickers.len().max(self.balances.len());
        if self.scroll_offset + 1 < max_items {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn get_crypto_price(&self, symbol: &str) -> Option<f64> {
        let sym_norm = normalize_crypto_symbol(symbol);
        for t in &self.tickers {
            if normalize_crypto_symbol(&t.symbol) == sym_norm
                && let Some(p) = t.price_num {
                    return Some(p);
                }
        }
        None
    }

    pub fn update_balance_values(&mut self) {
        for i in 0..self.balances.len() {
            let symbol = self.balances[i].symbol.clone();
            let amount = self.balances[i].amount;
            let norm = normalize_crypto_symbol(&symbol);
            if norm == "USD" {
                self.balances[i].value_usd = amount;
            } else if let Some(price) = self.get_crypto_price(&symbol) {
                self.balances[i].value_usd = amount * price;
            }
        }
    }
}

fn normalize_crypto_symbol(sym: &str) -> String {
    match sym.to_uppercase().as_str() {
        "XMR" | "MONERO" => "XMR".to_string(),
        "BTC" | "BITCOIN" => "BTC".to_string(),
        "ETH" | "ETHEREUM" => "ETH".to_string(),
        "BAT" | "BASIC-ATTENTION-TOKEN" => "BAT".to_string(),
        "USDC" | "USD" => "USD".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_initialization_and_scroll() {
        let cryptos = vec!["https://coinmarketcap.com/currencies/bitcoin/".to_string()];
        let stocks = vec!["GME".to_string(), "SOFI".to_string()];

        let mut state = AppState::new(cryptos, stocks);
        assert_eq!(state.tickers.len(), 3);
        assert_eq!(state.tickers[0].symbol, "GME");
        assert_eq!(state.tickers[1].symbol, "SOFI");
        assert_eq!(state.tickers[2].symbol, "BITCOIN");

        assert_eq!(state.scroll_offset, 0);
        state.scroll_down();
        assert_eq!(state.scroll_offset, 1);
        state.scroll_up();
        assert_eq!(state.scroll_offset, 0);
        state.scroll_up();
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_balance_calculation() {
        let mut state = AppState::new(vec![], vec![]);
        state.balances.push(BalanceItem {
            symbol: "USD".to_string(),
            amount: 100.0,
            value_usd: 100.0,
        });
        state.balances.push(BalanceItem {
            symbol: "BTC".to_string(),
            amount: 0.5,
            value_usd: 35000.0,
        });

        assert_eq!(state.total_balance_usd(), 35100.0);
    }
}
