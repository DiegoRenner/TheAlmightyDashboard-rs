use std::time::Instant;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct TickerItem {
    pub symbol: String,
    pub price_str: String,
    pub price_num: Option<f64>,
    pub last_success: Option<Instant>,
    pub last_gathered: Option<std::time::SystemTime>,
    pub delay_ms: u64,
    pub is_crypto: bool,
}

use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq)]
pub struct OutdatedField {
    pub name: String,
    pub is_session: bool,
    pub is_stale: bool,
    pub is_quote: bool,
    pub elapsed_secs: u64,
    pub gathered_time: chrono::DateTime<chrono::Local>,
}

impl OutdatedField {
    #[allow(dead_code)]
    pub fn age_display(&self) -> String {
        let s = self.elapsed_secs;
        if s < 60 {
            format!("{}s", s)
        } else if s < 3600 {
            format!("{}m {:02}s", s / 60, s % 60)
        } else if s < 86400 {
            format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
        } else {
            format!("{}d {:02}h", s / 86400, (s % 86400) / 3600)
        }
    }

    pub fn time_display(&self) -> String {
        self.gathered_time.format("%d.%m.%Y %H:%M:%S").to_string()
    }

    pub fn status_color(&self) -> Color {
        if self.is_stale {
            Color::LightRed
        } else if self.is_session {
            Color::LightCyan
        } else {
            Color::Yellow
        }
    }
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
            last_gathered: None,
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
    pub aud_to_chf: f64,
}

impl Default for FxRates {
    fn default() -> Self {
        Self {
            usd_to_chf: 0.81,
            eur_to_chf: 0.94,
            gbp_to_chf: 1.09,
            aud_to_chf: 0.584,
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
            "AUD" => amount * self.aud_to_chf,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    Live,
    Stale,
}

#[derive(Debug)]
pub struct AppState {
    pub tickers: Vec<TickerItem>,
    pub balances: Vec<BalanceItem>,
    pub fx_rates: FxRates,
    pub price_cache: std::collections::HashMap<String, f64>,
    pub account_sync: std::collections::HashMap<String, SyncStatus>,
    pub account_last_gathered: std::collections::HashMap<String, std::time::SystemTime>,
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
            account_sync: std::collections::HashMap::new(),
            account_last_gathered: std::collections::HashMap::new(),
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
            } else if self.balances[i].category == AccountCategory::Crypto
                && let Some(price) = self.get_crypto_price(&symbol)
            {
                self.balances[i].value_native = amount * price;
                self.balances[i].native_currency = "USD".to_string();
            }
            let native_curr = self.balances[i].native_currency.clone();
            let val_nat = self.balances[i].value_native;
            if self.balances[i].account != "SQ" {
                self.balances[i].value_chf = fx.to_chf(&native_curr, val_nat);
            }
        }
    }

    #[allow(dead_code)]
    pub fn mark_account_live(&mut self, account: &str) {
        self.account_sync.insert(account.to_string(), SyncStatus::Live);
    }

    pub fn mark_account_stale(&mut self, account: &str) {
        self.account_sync.insert(account.to_string(), SyncStatus::Stale);
    }

    pub fn is_account_stale(&self, account: &str) -> bool {
        matches!(self.account_sync.get(account), Some(SyncStatus::Stale))
    }

    pub fn is_session_dependent(account: &str) -> bool {
        matches!(account, "UH" | "FP" | "SQ" | "REV" | "UBS")
    }

    pub fn update_account_balances(&mut self, account: &str, new_items: Vec<BalanceItem>) {
        self.update_account_balances_with_time(account, new_items, std::time::SystemTime::now());
    }

    pub fn update_account_balances_with_time(
        &mut self,
        account: &str,
        new_items: Vec<BalanceItem>,
        gathered_at: std::time::SystemTime,
    ) {
        self.account_sync.insert(account.to_string(), SyncStatus::Live);
        self.account_last_gathered.insert(account.to_string(), gathered_at);
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
                let ts_path = parent.join("account_timestamps.json");
                let map: std::collections::HashMap<String, u64> = self
                    .account_last_gathered
                    .iter()
                    .filter_map(|(acc, t)| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| (acc.clone(), d.as_secs()))
                    })
                    .collect();
                if let Ok(json) = serde_json::to_string_pretty(&map) {
                    let _ = std::fs::write(ts_path, json);
                }
            }
            if let Ok(json) = serde_json::to_string_pretty(&self.balances) {
                let _ = std::fs::write(&path, json);
            }
        }
    }

    pub fn load_balance_cache(&mut self) {
        if let Some(path) = balance_cache_path() {
            let mut loaded_timestamps: std::collections::HashMap<String, std::time::SystemTime> =
                std::collections::HashMap::new();
            if let Some(parent) = path.parent() {
                let ts_path = parent.join("account_timestamps.json");
                if let Ok(ts_json) = std::fs::read_to_string(&ts_path)
                    && let Ok(map) =
                        serde_json::from_str::<std::collections::HashMap<String, u64>>(&ts_json)
                {
                    for (acc, secs) in map {
                        loaded_timestamps.insert(
                            acc,
                            std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs),
                        );
                    }
                }
            }

            if let Ok(json) = std::fs::read_to_string(&path)
                && let Ok(items) = serde_json::from_str::<Vec<BalanceItem>>(&json)
                && !items.is_empty()
            {
                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .unwrap_or_else(|_| std::time::SystemTime::now());

                self.balances = items;
                for b in &self.balances {
                    self.account_sync
                        .entry(b.account.clone())
                        .or_insert(SyncStatus::Stale);
                    let ts = loaded_timestamps
                        .get(&b.account)
                        .copied()
                        .unwrap_or(mtime);
                    self.account_last_gathered
                        .entry(b.account.clone())
                        .or_insert(ts);
                }
                self.update_balance_values();
            }
        }
    }

    pub fn most_outdated_account(&self) -> Option<OutdatedField> {
        let now = std::time::SystemTime::now();
        let mut candidates: Vec<OutdatedField> = Vec::new();
        let mut accounts_seen = std::collections::HashSet::new();

        for b in &self.balances {
            if accounts_seen.insert(b.account.clone()) {
                let time = self.account_last_gathered.get(&b.account).copied().unwrap_or(now);
                let elapsed = now.duration_since(time).unwrap_or_default().as_secs();
                let is_sess = Self::is_session_dependent(&b.account);
                let is_stale = self.is_account_stale(&b.account);
                let name = if is_sess {
                    format!("{}*", b.account)
                } else {
                    b.account.clone()
                };

                candidates.push(OutdatedField {
                    name,
                    is_session: is_sess,
                    is_stale,
                    is_quote: false,
                    elapsed_secs: elapsed,
                    gathered_time: time.into(),
                });
            }
        }

        candidates.into_iter().max_by_key(|c| c.elapsed_secs)
    }

    pub fn most_outdated_ticker(&self) -> Option<OutdatedField> {
        let now = std::time::SystemTime::now();
        let mut candidates: Vec<OutdatedField> = Vec::new();

        for t in &self.tickers {
            if let Some(time) = t.last_gathered {
                let elapsed = now.duration_since(time).unwrap_or_default().as_secs();
                candidates.push(OutdatedField {
                    name: t.symbol.clone(),
                    is_session: false,
                    is_stale: false,
                    is_quote: true,
                    elapsed_secs: elapsed,
                    gathered_time: time.into(),
                });
            }
        }

        candidates.into_iter().max_by_key(|c| c.elapsed_secs)
    }

    pub fn most_outdated_field(&self) -> Option<OutdatedField> {
        let mut candidates: Vec<OutdatedField> = Vec::new();

        if let Some(acc) = self.most_outdated_account() {
            candidates.push(acc);
        }
        if let Some(tick) = self.most_outdated_ticker() {
            candidates.push(tick);
        }

        candidates.into_iter().max_by_key(|c| c.elapsed_secs)
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
            aud_to_chf: 0.58,
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
            aud_to_chf: 0.584,
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

    #[test]
    fn test_sync_status_live_and_stale() {
        let mut state = AppState::new(vec![], vec![]);
        assert!(AppState::is_session_dependent("UH"));
        assert!(AppState::is_session_dependent("FP"));
        assert!(AppState::is_session_dependent("SQ"));
        assert!(AppState::is_session_dependent("REV"));
        assert!(AppState::is_session_dependent("UBS"));
        assert!(!AppState::is_session_dependent("CB"));
        assert!(!AppState::is_session_dependent("IB"));
        assert!(!AppState::is_session_dependent("ST"));
        assert!(!AppState::is_session_dependent("MW"));
        assert!(!AppState::is_session_dependent("CW"));
        assert!(!AppState::is_session_dependent("Kraken"));

        assert!(!state.is_account_stale("UH"));
        state.mark_account_stale("UH");
        assert!(state.is_account_stale("UH"));

        // update_account_balances marks live
        state.update_account_balances("UH", vec![BalanceItem {
            account: "UH".to_string(),
            category: AccountCategory::Crypto,
            symbol: "BAT".to_string(),
            amount: 100.0,
            native_currency: "USD".to_string(),
            value_native: 20.0,
            value_chf: 16.0,
        }]);
        assert!(!state.is_account_stale("UH"));
        assert_eq!(state.account_sync.get("UH"), Some(&SyncStatus::Live));

        state.mark_account_stale("UH");
        assert!(state.is_account_stale("UH"));
        state.mark_account_live("UH");
        assert!(!state.is_account_stale("UH"));
    }

    #[test]
    fn test_outdated_field_displays_and_colors() {
        let now = chrono::Local::now();
        let field_sec = OutdatedField {
            name: "BTC".to_string(),
            is_session: false,
            is_stale: false,
            is_quote: true,
            elapsed_secs: 42,
            gathered_time: now,
        };
        assert_eq!(field_sec.age_display(), "42s");
        assert_eq!(field_sec.time_display(), now.format("%d.%m.%Y %H:%M:%S").to_string());
        assert_eq!(field_sec.status_color(), ratatui::style::Color::Yellow);

        let field_min = OutdatedField {
            name: "UBS*".to_string(),
            is_session: true,
            is_stale: false,
            is_quote: false,
            elapsed_secs: 863, // 14m 23s
            gathered_time: now,
        };
        assert_eq!(field_min.age_display(), "14m 23s");
        assert_eq!(field_min.status_color(), ratatui::style::Color::LightCyan);

        let field_hour = OutdatedField {
            name: "FP*".to_string(),
            is_session: true,
            is_stale: true,
            is_quote: false,
            elapsed_secs: 7320, // 2h 02m
            gathered_time: now,
        };
        assert_eq!(field_hour.age_display(), "2h 02m");
        assert_eq!(field_hour.status_color(), ratatui::style::Color::LightRed);

        let field_day = OutdatedField {
            name: "IB".to_string(),
            is_session: false,
            is_stale: false,
            is_quote: false,
            elapsed_secs: 100000,
            gathered_time: now,
        };
        assert_eq!(field_day.age_display(), "1d 03h");
        assert_eq!(field_day.time_display(), now.format("%d.%m.%Y %H:%M:%S").to_string());
    }

    #[test]
    fn test_most_outdated_field_selection() {
        let mut state = AppState::new(vec![], vec![]);
        let now = std::time::SystemTime::now();
        let ten_mins_ago = now - std::time::Duration::from_secs(600);
        let five_mins_ago = now - std::time::Duration::from_secs(300);

        state.balances.push(BalanceItem {
            account: "IB".to_string(),
            category: AccountCategory::Stocks,
            symbol: "AAPL".to_string(),
            amount: 10.0,
            native_currency: "USD".to_string(),
            value_native: 1500.0,
            value_chf: 1200.0,
        });
        state.account_last_gathered.insert("IB".to_string(), five_mins_ago);

        state.balances.push(BalanceItem {
            account: "UBS".to_string(),
            category: AccountCategory::Cash,
            symbol: "CHF".to_string(),
            amount: 100.0,
            native_currency: "CHF".to_string(),
            value_native: 100.0,
            value_chf: 100.0,
        });
        state.account_last_gathered.insert("UBS".to_string(), ten_mins_ago);

        // Account UBS is older than IB
        let oldest_acc = state.most_outdated_account().unwrap();
        assert_eq!(oldest_acc.name, "UBS*");
        assert!(oldest_acc.elapsed_secs >= 600);

        // Add ticker that is even older (15 mins ago)
        let fifteen_mins_ago = now - std::time::Duration::from_secs(900);
        let mut ticker = TickerItem::new("BTC-USD", true);
        ticker.last_gathered = Some(fifteen_mins_ago);
        state.tickers.push(ticker);

        let oldest = state.most_outdated_field().unwrap();
        assert_eq!(oldest.name, "BTC-USD");
        assert!(oldest.is_quote);
        assert!(oldest.elapsed_secs >= 900);
    }
}


