use std::time::Instant;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountCategory {
    Crypto,
    Stocks,
    Cash,
    Retirement,
}

impl std::fmt::Display for AccountCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountCategory::Crypto => write!(f, "Crypto"),
            AccountCategory::Stocks => write!(f, "Stocks"),
            AccountCategory::Cash => write!(f, "Cash"),
            AccountCategory::Retirement => write!(f, "Retirement"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FxRates {
    pub usd_to_chf: f64,
    pub eur_to_chf: f64,
    pub gbp_to_chf: f64,
}

impl Default for FxRates {
    fn default() -> Self {
        Self {
            usd_to_chf: 0.81,
            eur_to_chf: 0.94,
            gbp_to_chf: 1.09,
        }
    }
}

impl FxRates {
    pub fn to_chf(self, currency: &str, amount: f64) -> f64 {
        match currency.to_uppercase().as_str() {
            "CHF" => amount,
            "USD" => amount * self.usd_to_chf,
            "EUR" => amount * self.eur_to_chf,
            "GBP" => amount * self.gbp_to_chf,
            _ => amount * self.usd_to_chf,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BalanceItem {
    pub account: String,
    pub category: AccountCategory,
    pub symbol: String,
    pub amount: f64,
    pub native_currency: String,
    pub value_native: f64,
    pub value_chf: f64,
}

#[derive(Debug)]
pub struct AppState {
    pub tickers: Vec<TickerItem>,
    pub balances: Vec<BalanceItem>,
    pub fx_rates: FxRates,
    pub price_cache: std::collections::HashMap<String, f64>,
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
            fx_rates: FxRates::default(),
            price_cache: std::collections::HashMap::new(),
            scroll_offset: 0,
            should_quit: false,
        }
    }

    pub fn total_balance_usd(&self) -> f64 {
        if self.fx_rates.usd_to_chf > 0.0 {
            self.total_net_worth_chf() / self.fx_rates.usd_to_chf
        } else {
            0.0
        }
    }

    pub fn crypto_total_chf(&self) -> f64 {
        self.balances
            .iter()
            .filter(|b| b.category == AccountCategory::Crypto)
            .map(|b| b.value_chf)
            .sum()
    }

    pub fn stocks_total_chf(&self) -> f64 {
        self.balances
            .iter()
            .filter(|b| b.category == AccountCategory::Stocks)
            .map(|b| b.value_chf)
            .sum()
    }

    pub fn cash_total_chf(&self) -> f64 {
        self.balances
            .iter()
            .filter(|b| b.category == AccountCategory::Cash)
            .map(|b| b.value_chf)
            .sum()
    }

    pub fn stocks_and_cash_total_chf(&self) -> f64 {
        self.stocks_total_chf() + self.cash_total_chf()
    }

    pub fn retirement_total_chf(&self) -> f64 {
        self.balances
            .iter()
            .filter(|b| b.category == AccountCategory::Retirement)
            .map(|b| b.value_chf)
            .sum()
    }

    pub fn total_net_worth_chf(&self) -> f64 {
        self.balances.iter().map(|b| b.value_chf).sum()
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
        if sym_norm == "USD" {
            return Some(1.0);
        }
        if let Some(&p) = self.price_cache.get(&sym_norm) {
            return Some(p);
        }
        for t in &self.tickers {
            if normalize_crypto_symbol(&t.symbol) == sym_norm
                && let Some(p) = t.price_num {
                    return Some(p);
                }
        }
        None
    }

    pub fn set_crypto_price(&mut self, symbol: &str, price: f64) {
        let sym_norm = normalize_crypto_symbol(symbol);
        self.price_cache.insert(sym_norm, price);
    }

    pub fn update_balance_values(&mut self) {
        let fx = self.fx_rates;
        for i in 0..self.balances.len() {
            let symbol = self.balances[i].symbol.clone();
            let amount = self.balances[i].amount;
            let norm = normalize_crypto_symbol(&symbol);
            if norm == "USD" {
                self.balances[i].value_native = amount;
                self.balances[i].native_currency = "USD".to_string();
            } else if norm == "EUR" {
                self.balances[i].value_native = amount;
                self.balances[i].native_currency = "EUR".to_string();
            } else if norm == "GBP" {
                self.balances[i].value_native = amount;
                self.balances[i].native_currency = "GBP".to_string();
            } else if norm == "CHF" {
                self.balances[i].value_native = amount;
                self.balances[i].native_currency = "CHF".to_string();
            } else if let Some(price) = self.get_crypto_price(&symbol) {
                self.balances[i].value_native = amount * price;
                self.balances[i].native_currency = "USD".to_string();
            }
            let native_curr = self.balances[i].native_currency.clone();
            let val_nat = self.balances[i].value_native;
            self.balances[i].value_chf = fx.to_chf(&native_curr, val_nat);
        }
    }

    pub fn update_account_balances(&mut self, account: &str, new_items: Vec<BalanceItem>) {
        let mut updated = Vec::new();
        let mut inserted = false;
        for b in self.balances.drain(..) {
            if b.account == account {
                if !inserted {
                    updated.extend(new_items.clone());
                    inserted = true;
                }
            } else {
                updated.push(b);
            }
        }
        if !inserted {
            updated.extend(new_items);
        }
        self.balances = updated;
        self.update_balance_values();
        self.save_balance_cache();
    }

    pub fn save_balance_cache(&self) {
        if self.balances.is_empty() {
            return;
        }
        if let Some(path) = balance_cache_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&self.balances) {
                let _ = std::fs::write(&path, json);
            }
        }
    }

    pub fn load_balance_cache(&mut self) {
        if let Some(path) = balance_cache_path()
            && let Ok(json) = std::fs::read_to_string(&path)
            && let Ok(items) = serde_json::from_str::<Vec<BalanceItem>>(&json)
            && !items.is_empty()
        {
            self.balances = items;
            self.update_balance_values();
        }
    }
}

pub fn balance_cache_path() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        if let Ok(path) = std::env::var("DASHBOARD_CACHE_FILE") {
            return Some(std::path::PathBuf::from(path));
        }
        if let Ok(home) = std::env::var("HOME") {
            Some(std::path::PathBuf::from(home).join(".cache/the-almighty-dashboard/balances.json"))
        } else {
            Some(std::path::PathBuf::from(".balances_cache.json"))
        }
    }
}

pub fn normalize_crypto_symbol(sym: &str) -> String {
    match sym.to_uppercase().as_str() {
        "XMR" | "MONERO" => "XMR".to_string(),
        "BTC" | "BITCOIN" => "BTC".to_string(),
        "ETH" | "ETHEREUM" => "ETH".to_string(),
        "BAT" | "BASIC-ATTENTION-TOKEN" => "BAT".to_string(),
        "XCH" | "CHIA" | "CHIA-NETWORK" => "XCH".to_string(),
        "DOGE" | "DOGECOIN" => "DOGE".to_string(),
        "USDC" | "USD" | "USDT" => "USD".to_string(),
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
        state.fx_rates = FxRates {
            usd_to_chf: 0.80,
            eur_to_chf: 0.90,
            gbp_to_chf: 1.10,
        };
        state.balances.push(BalanceItem {
            account: "CB".to_string(),
            category: AccountCategory::Crypto,
            symbol: "USD".to_string(),
            amount: 100.0,
            native_currency: "USD".to_string(),
            value_native: 100.0,
            value_chf: 80.0,
        });
        state.balances.push(BalanceItem {
            account: "MW".to_string(),
            category: AccountCategory::Crypto,
            symbol: "BTC".to_string(),
            amount: 0.5,
            native_currency: "USD".to_string(),
            value_native: 35000.0,
            value_chf: 28000.0,
        });
        state.balances.push(BalanceItem {
            account: "ST".to_string(),
            category: AccountCategory::Cash,
            symbol: "GBP".to_string(),
            amount: 1000.0,
            native_currency: "GBP".to_string(),
            value_native: 1000.0,
            value_chf: 1100.0,
        });

        assert_eq!(state.crypto_total_chf(), 28080.0);
        assert_eq!(state.cash_total_chf(), 1100.0);
        assert_eq!(state.stocks_and_cash_total_chf(), 1100.0);
        assert_eq!(state.total_net_worth_chf(), 29180.0);
    }

    #[test]
    fn test_update_balance_values_with_tickers() {
        let mut state = AppState::new(
            vec!["https://coinmarketcap.com/currencies/monero/".to_string(), "https://coinmarketcap.com/currencies/ethereum/".to_string()],
            vec![]
        );
        state.fx_rates = FxRates {
            usd_to_chf: 0.81,
            eur_to_chf: 0.94,
            gbp_to_chf: 1.09,
        };

        state.balances.push(BalanceItem {
            account: "MW".to_string(),
            category: AccountCategory::Crypto,
            symbol: "XMR".to_string(),
            amount: 9.38,
            native_currency: "USD".to_string(),
            value_native: 0.0,
            value_chf: 0.0,
        });
        state.balances.push(BalanceItem {
            account: "UH".to_string(),
            category: AccountCategory::Crypto,
            symbol: "ETH".to_string(),
            amount: 0.00865,
            native_currency: "USD".to_string(),
            value_native: 0.0,
            value_chf: 0.0,
        });

        // BEFORE tickers fetch prices:
        state.update_balance_values();
        assert_eq!(state.balances[0].value_chf, 0.0);

        // AFTER ticker prices fetched:
        state.tickers[0].symbol = "XMR".to_string();
        state.tickers[0].price_num = Some(530.0);
        state.tickers[1].symbol = "ETH".to_string();
        state.tickers[1].price_num = Some(2450.0);

        state.update_balance_values();
        println!("After ticker prices: XMR val_chf={}, ETH val_chf={}", state.balances[0].value_chf, state.balances[1].value_chf);
        assert!(state.balances[0].value_chf > 0.0);
        assert!(state.balances[1].value_chf > 0.0);
    }

    #[test]
    fn test_update_account_balances_preserves_other_accounts() {
        let mut state = AppState::new(vec![], vec![]);
        state.update_account_balances("ST", vec![BalanceItem {
            account: "ST".to_string(),
            category: AccountCategory::Cash,
            symbol: "GBP".to_string(),
            amount: 5.31,
            native_currency: "GBP".to_string(),
            value_native: 5.31,
            value_chf: 5.31 * 1.09,
        }]);
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].account, "ST");

        // Now update another account "UH":
        state.update_account_balances("UH", vec![BalanceItem {
            account: "UH".to_string(),
            category: AccountCategory::Crypto,
            symbol: "ETH".to_string(),
            amount: 0.01,
            native_currency: "USD".to_string(),
            value_native: 25.0,
            value_chf: 20.0,
        }]);
        assert_eq!(state.balances.len(), 2);
        // ST should still be preserved:
        assert!(state.balances.iter().any(|b| b.account == "ST" && b.amount == 5.31));

        // Now update ST with new balance:
        state.update_account_balances("ST", vec![BalanceItem {
            account: "ST".to_string(),
            category: AccountCategory::Cash,
            symbol: "GBP".to_string(),
            amount: 10.0,
            native_currency: "GBP".to_string(),
            value_native: 10.0,
            value_chf: 10.9,
        }]);
        assert_eq!(state.balances.len(), 2);
        assert!(state.balances.iter().any(|b| b.account == "ST" && b.amount == 10.0));
        assert!(state.balances.iter().any(|b| b.account == "UH" && b.amount == 0.01));
    }
}


