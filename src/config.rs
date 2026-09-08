use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrVec {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            StringOrVec::Single(s) => {
                if s.is_empty() {
                    vec![]
                } else {
                    vec![s]
                }
            }
            StringOrVec::Multiple(v) => v,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default, alias = "crypto_symbols")]
    pub coinmarketcap_urls: Vec<String>,

    #[serde(default, alias = "stock_symbols")]
    pub marketwatch_urls: Vec<String>,

    #[serde(default)]
    pub monero_wallet_address: Option<StringOrVec>,

    #[serde(default)]
    pub uphold_token: Option<String>,

    #[serde(default)]
    pub coinbase_api_key: Option<String>,

    #[serde(default)]
    pub coinbase_api_secret: Option<String>,

    #[serde(default)]
    pub chia_db_path: Option<String>,

    #[serde(default)]
    pub starling_token: Option<String>,

    #[serde(default)]
    pub kraken_api_key: Option<String>,

    #[serde(default)]
    pub kraken_api_secret: Option<String>,

    #[serde(default)]
    pub ibkr_flex_token: Option<String>,

    #[serde(default)]
    pub ibkr_query_id: Option<String>,

    #[serde(default)]
    pub finpension_token: Option<String>,

    /// Manual cash fields managed from the [c] modal; written back by save_custom_cash_to_config.
    #[serde(default)]
    pub cash_misc: Vec<CustomCashItem>,

    /// File this config was loaded from (and is saved to); not part of the JSON.
    #[serde(skip)]
    pub path: PathBuf,

    #[serde(default = "default_update_freq")]
    pub update_frequency_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomCashItem {
    pub name: String,
    pub amount: f64,
    pub currency: String,
}

/// Account codes the automated providers emit; they are never editable as manual cash fields.
pub const RESERVED_AUTOMATED_ACCOUNTS: &[&str] = &["UH", "MW", "CW", "CB", "IB", "KRAKEN", "FP", "SQ", "REV", "UBS", "ST"];

pub fn is_reserved_automated_account(name: &str) -> bool {
    let upper = name.trim().to_uppercase();
    RESERVED_AUTOMATED_ACCOUNTS.contains(&upper.as_str())
}

/// Currencies FxRates::to_chf converts; anything else would silently be valued at the USD rate.
pub const SUPPORTED_CURRENCIES: &[&str] = &["CHF", "USD", "EUR", "GBP", "AUD"];

pub fn validate_custom_cash_item(item: &CustomCashItem) -> Result<(), String> {
    if item.name.trim().is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if is_reserved_automated_account(&item.name) {
        return Err(format!(
            "'{}' is an automated account and cannot be added or modified as a custom cash field",
            item.name
        ));
    }
    if !item.amount.is_finite() {
        return Err("Amount must be a finite number".to_string());
    }
    if !SUPPORTED_CURRENCIES.contains(&item.currency.as_str()) {
        return Err(format!(
            "Unsupported currency '{}'. Use one of {}",
            item.currency,
            SUPPORTED_CURRENCIES.join(", ")
        ));
    }
    Ok(())
}

/// A numeric token like "1234.50", "£1,500", "2'000$" -> (amount, currency implied by its symbol).
/// Exactly one currency symbol is allowed, so a contradictory "£500$" is not silently read as USD.
fn amount_token(word: &str) -> Option<(f64, Option<&'static str>)> {
    let mut currency = None;
    let mut s = word;
    for (symbol, code) in [('£', "GBP"), ('$', "USD"), ('€', "EUR")] {
        if let Some(rest) = s.strip_prefix(symbol).or_else(|| s.strip_suffix(symbol)) {
            if currency.is_some() {
                return None;
            }
            currency = Some(code);
            s = rest;
        }
    }
    if !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // ',' and ' are thousands separators only: never after the decimal point, at most 3 digits in the
    // leading group and exactly 3 in every later one. So a decimal comma ("1.500,50", "1500,50",
    // "1234,567") is rejected rather than silently read as 1.5005 / 150050 / 1234567.
    let (int_part, _) = s.split_once('.').unwrap_or((s, ""));
    if s.rfind([',', '\'']) > s.find('.') && s.contains('.') {
        return None;
    }
    let mut groups = int_part.split([',', '\'']);
    let leading = groups.next().unwrap_or_default();
    if int_part.contains([',', '\'']) && (leading.is_empty() || leading.len() > 3) {
        return None;
    }
    if groups.any(|g| g.len() != 3) {
        return None;
    }
    let value: f64 = s.replace([',', '\''], "").parse().ok()?;
    value.is_finite().then_some((value, currency))
}

/// A three-letter alphabetic word, i.e. something shaped like a currency code.
fn is_currency_code(word: &str) -> bool {
    word.len() == 3 && word.chars().all(|c| c.is_ascii_alphabetic())
}

/// Splits free text into (amount, currency, leftover words). The amount is the last numeric token.
/// The currency comes from the amount's own symbol, else a code word directly after it, else a
/// supported code directly before it; everything else is leftover and forms the name.
fn split_entry(input: &str) -> Result<(f64, String, Vec<String>), String> {
    let words: Vec<&str> = input
        .split_whitespace()
        .map(|w| w.trim_matches([':', ',', ';']))
        .filter(|w| !w.is_empty())
        .collect();
    let amount_idx = words
        .iter()
        .rposition(|w| amount_token(w).is_some())
        .ok_or_else(|| format!("Could not find an amount in '{}'. Example: 'Chase 1234.50 GBP'", input.trim()))?;
    let (amount, symbol_currency) = amount_token(words[amount_idx]).expect("checked by rposition");

    let unsupported = |code: &str| {
        format!("Unsupported currency '{code}'. Use one of {}", SUPPORTED_CURRENCIES.join(", "))
    };
    let mut currency = symbol_currency.map(str::to_string);
    let mut consumed = None;

    // a code right after the amount is always meant as the currency, so an unsupported one is an error
    if let Some(word) = words.get(amount_idx + 1).filter(|w| is_currency_code(w)) {
        let upper = word.to_uppercase();
        if !SUPPORTED_CURRENCIES.contains(&upper.as_str()) {
            return Err(unsupported(&upper));
        }
        match &currency {
            Some(sym) if *sym != upper => return Err(format!("'{}' contradicts '{upper}'", words[amount_idx])),
            _ => currency = Some(upper),
        }
        consumed = Some(amount_idx + 1);
    }

    // "EUR 1000": a supported code right before the amount also names the currency
    if currency.is_none()
        && let Some(i) = amount_idx.checked_sub(1)
        && let Some(word) = words.get(i).filter(|w| is_currency_code(w))
        && SUPPORTED_CURRENCIES.contains(&word.to_uppercase().as_str())
    {
        currency = Some(word.to_uppercase());
        consumed = Some(i);
    }

    let leftover = words
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != amount_idx && Some(*i) != consumed)
        .map(|(_, w)| w.to_string())
        .collect();
    Ok((amount, currency.unwrap_or_else(|| "CHF".to_string()), leftover))
}

/// "Chase 1234.50 GBP" -> a validated item; words that are neither amount nor currency form the name.
pub fn parse_custom_cash_entry(input: &str) -> Result<CustomCashItem, String> {
    if input.trim().is_empty() {
        return Err("Please enter a name, amount, and currency (e.g. 'Chase 1234.50 GBP')".to_string());
    }
    let (amount, currency, leftover) = split_entry(input)?;
    if leftover.is_empty() {
        return Err("Please include a name (e.g. 'Chase 1234.50 GBP')".to_string());
    }
    let item = CustomCashItem { name: leftover.join(" "), amount, currency };
    validate_custom_cash_item(&item)?;
    Ok(item)
}

/// "1500 GBP" -> (1500.0, "GBP"); any other words are an error.
pub fn parse_amount_currency(input: &str) -> Result<(f64, String), String> {
    if input.trim().is_empty() {
        return Err("Please enter an amount and currency (e.g. 1500 GBP)".to_string());
    }
    let (amount, currency, leftover) = split_entry(input)?;
    if !leftover.is_empty() {
        return Err(format!("Unexpected text '{}'; enter amount and currency only", leftover.join(" ")));
    }
    Ok((amount, currency))
}

/// Rewrites only the `cash_misc` key of the config file at `path` (created if missing). Entries the
/// app rejected as invalid are kept, so saving never deletes something the user typed by hand.
pub fn save_custom_cash_to_config(path: &Path, items: &[CustomCashItem]) -> Result<(), String> {
    let mut val: serde_json::Value = match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).map_err(|e| format!("{}: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut merged = items.to_vec();
    if let Some(existing) = val.get("cash_misc").and_then(|v| serde_json::from_value::<Vec<CustomCashItem>>(v.clone()).ok()) {
        merged.extend(existing.into_iter().filter(|c| validate_custom_cash_item(c).is_err()));
    }
    val["cash_misc"] = serde_json::to_value(&merged).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let pretty = serde_json::to_string_pretty(&val).map_err(|e| e.to_string())?;
    fs::write(&tmp, pretty).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn default_update_freq() -> f64 {
    5.0
}

impl Config {
    pub fn load_or_default<P: AsRef<Path>>(path_opt: Option<P>) -> Result<Self, Box<dyn std::error::Error>> {
        let explicit = path_opt.is_some();
        let path: PathBuf = match path_opt {
            Some(p) => p.as_ref().to_path_buf(),
            None => ["config.json", "config_no_accounts.json"]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from("config.json")),
        };
        let mut config: Config = if explicit || path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?)?
        } else {
            Config::default()
        };
        config.path = path;
        Ok(config)
    }

    /// Config items that pass validation; invalid ones are dropped rather than mis-valued or hijacking a
    /// provider, and a repeated name keeps its last entry so names stay unique like in the modal.
    pub fn custom_cash(&self) -> Vec<CustomCashItem> {
        let mut items: Vec<CustomCashItem> = Vec::new();
        for c in &self.cash_misc {
            let c = CustomCashItem { currency: c.currency.to_uppercase(), ..c.clone() };
            if validate_custom_cash_item(&c).is_err() {
                continue;
            }
            match items.iter().position(|i| i.name.eq_ignore_ascii_case(&c.name)) {
                Some(pos) => items[pos] = c,
                None => items.push(c),
            }
        }
        items
    }

    /// Names of config entries custom_cash() dropped, so the user can be told why they are missing.
    pub fn rejected_cash_names(&self) -> Vec<String> {
        self.cash_misc
            .iter()
            .map(|c| CustomCashItem { currency: c.currency.to_uppercase(), ..c.clone() })
            .filter(|c| validate_custom_cash_item(c).is_err())
            .map(|c| c.name)
            .collect()
    }

    pub fn get_monero_addresses(&self) -> Vec<String> {
        self.monero_wallet_address
            .as_ref()
            .map(|v| v.clone().into_vec())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_no_accounts() {
        let config = Config::load_or_default(Some("config_no_accounts.json")).expect("failed to load config");
        assert!(!config.coinmarketcap_urls.is_empty());
        assert!(!config.marketwatch_urls.is_empty());
        assert_eq!(config.get_monero_addresses().len(), 0);
        assert!(config.uphold_token.is_none());
    }

    #[test]
    fn test_string_or_vec_parsing() {
        let json_single = r#"{"monero_wallet_address": "addr1"}"#;
        let c_single: Config = serde_json::from_str(json_single).unwrap();
        assert_eq!(c_single.get_monero_addresses(), vec!["addr1"]);

        let json_multi = r#"{"monero_wallet_address": ["addr1", "addr2"]}"#;
        let c_multi: Config = serde_json::from_str(json_multi).unwrap();
        assert_eq!(c_multi.get_monero_addresses(), vec!["addr1", "addr2"]);
    }

    #[test]
    fn test_parse_custom_cash_entry() {
        let e1 = parse_custom_cash_entry("Chase 1234.50 GBP").unwrap();
        assert_eq!((e1.name.as_str(), e1.amount, e1.currency.as_str()), ("Chase", 1234.50, "GBP"));

        let e2 = parse_custom_cash_entry("Chase UK: £1,500").unwrap();
        assert_eq!((e2.name.as_str(), e2.amount, e2.currency.as_str()), ("Chase UK", 1500.0, "GBP"));

        let e3 = parse_custom_cash_entry("Safe 500").unwrap();
        assert_eq!((e3.name.as_str(), e3.amount, e3.currency.as_str()), ("Safe", 500.0, "CHF"));

        let e4 = parse_custom_cash_entry("Emergency EUR 1000").unwrap();
        assert_eq!((e4.name.as_str(), e4.amount, e4.currency.as_str()), ("Emergency", 1000.0, "EUR"));

        // last numeric token is the amount, digits inside the name stay in the name
        let e5 = parse_custom_cash_entry("Account 2 1500 GBP").unwrap();
        assert_eq!((e5.name.as_str(), e5.amount), ("Account 2", 1500.0));

        // "Infinity" parses as f64 but has no digit, so it is a name
        let e6 = parse_custom_cash_entry("Infinity Bank 500 CHF").unwrap();
        assert_eq!((e6.name.as_str(), e6.amount), ("Infinity Bank", 500.0));

        // a name is mandatory, and decimal commas are rejected rather than mis-parsed
        assert!(parse_custom_cash_entry("1500 GBP").unwrap_err().contains("include a name"));
        assert!(parse_custom_cash_entry("Chase 1.500,50 EUR").is_err());
        assert!(parse_custom_cash_entry("Chase 1234,567").is_err());
        assert_eq!(parse_custom_cash_entry("Chase 1'234.50 GBP").unwrap().amount, 1234.50);

        // a symbol plus its own code is redundant, not a contradiction
        let e7 = parse_custom_cash_entry("Chase £1,500 GBP").unwrap();
        assert_eq!((e7.name.as_str(), e7.amount, e7.currency.as_str()), ("Chase", 1500.0, "GBP"));
        assert!(parse_custom_cash_entry("Chase $500 GBP").unwrap_err().contains("contradicts"));

        // codes away from the amount stay part of the name
        let e8 = parse_custom_cash_entry("Gbp Account 5").unwrap();
        assert_eq!((e8.name.as_str(), e8.currency.as_str()), ("Gbp Account", "CHF"));
        // ...but one right after the amount is meant as the currency
        assert!(parse_custom_cash_entry("Wallet 500 DKK").unwrap_err().contains("Unsupported currency"));
        // a three-letter name before the amount is still a name
        assert_eq!(parse_custom_cash_entry("ATM 500").unwrap().name, "ATM");

        // reserved provider accounts and unsupported currencies are rejected
        assert!(parse_custom_cash_entry("UBS 500 CHF").unwrap_err().contains("automated account"));
        assert!(parse_custom_cash_entry("ST 100 GBP").is_err());
        assert!(parse_custom_cash_entry("Wallet 100000 JPY").unwrap_err().contains("Unsupported currency"));
        assert!(parse_custom_cash_entry("").is_err());
        assert!(parse_custom_cash_entry("just words").is_err());
    }

    #[test]
    fn test_parse_amount_currency() {
        assert_eq!(parse_amount_currency("1500 GBP").unwrap(), (1500.0, "GBP".to_string()));
        assert_eq!(parse_amount_currency("1250.50").unwrap(), (1250.50, "CHF".to_string()));
        assert_eq!(parse_amount_currency("£500").unwrap(), (500.0, "GBP".to_string()));
        assert_eq!(parse_amount_currency("$2,500.25").unwrap(), (2500.25, "USD".to_string()));
        assert_eq!(parse_amount_currency("€300").unwrap(), (300.0, "EUR".to_string()));
        assert_eq!(parse_amount_currency("CHF 4000").unwrap(), (4000.0, "CHF".to_string()));
        assert_eq!(parse_amount_currency("5000 aud").unwrap(), (5000.0, "AUD".to_string()));
        assert!(parse_amount_currency("").is_err());
        assert!(parse_amount_currency("invalid").is_err());
        assert!(parse_amount_currency("1500 pounds").is_err());
        assert!(parse_amount_currency("Chase 1500 GBP").is_err());
        assert!(parse_amount_currency("1500 DKK").unwrap_err().contains("Unsupported currency"));
        assert!(parse_amount_currency("1500,50 EUR").is_err());
        assert!(parse_amount_currency("1,5").is_err());
        assert!(parse_amount_currency("1234,567").is_err());
        assert_eq!(parse_amount_currency("1,500,000 USD").unwrap().0, 1_500_000.0);
        assert_eq!(parse_amount_currency("£1500 GBP").unwrap(), (1500.0, "GBP".to_string()));
        assert!(parse_amount_currency("£500$").is_err());
    }

    #[test]
    fn test_config_custom_cash_list_validated() {
        let json_list = r#"{
            "cash_misc": [
                {"name": "Chase", "amount": 1234.50, "currency": "gbp"},
                {"name": "ST", "amount": 100.0, "currency": "GBP"},
                {"name": "Wallet", "amount": 5.0, "currency": "JPY"},
                {"name": "Physical Cash", "amount": 250.0, "currency": "CHF"},
                {"name": "chase", "amount": 5.0, "currency": "GBP"}
            ]
        }"#;
        let c: Config = serde_json::from_str(json_list).unwrap();
        let items = c.custom_cash();
        assert_eq!(items.len(), 2, "reserved names, unsupported currencies and repeated names are collapsed");
        assert_eq!((items[0].name.as_str(), items[0].amount, items[0].currency.as_str()), ("chase", 5.0, "GBP"));
        assert_eq!(items[1].name, "Physical Cash");

        let none: Config = serde_json::from_str("{}").unwrap();
        assert!(none.custom_cash().is_empty());
    }

    #[test]
    fn test_save_custom_cash_round_trip() {
        let path = std::env::temp_dir().join(format!("dashboard_cash_{}.json", std::process::id()));
        fs::write(&path, r#"{"starling_token": "keep-me"}"#).unwrap();
        let items = vec![CustomCashItem { name: "Chase".to_string(), amount: 1234.50, currency: "GBP".to_string() }];
        save_custom_cash_to_config(&path, &items).unwrap();

        let reloaded = Config::load_or_default(Some(&path)).unwrap();
        assert_eq!(reloaded.custom_cash(), items);
        assert_eq!(reloaded.starling_token.as_deref(), Some("keep-me"));
        assert_eq!(reloaded.path, path);

        // an entry the app rejects is kept on disk instead of being silently deleted by a save
        fs::write(
            &path,
            r#"{"cash_misc": [{"name": "Wallet", "amount": 5.0, "currency": "JPY"}]}"#,
        )
        .unwrap();
        save_custom_cash_to_config(&path, &items).unwrap();
        let after = Config::load_or_default(Some(&path)).unwrap();
        assert_eq!(after.custom_cash(), items);
        assert_eq!(after.rejected_cash_names(), vec!["Wallet".to_string()]);

        save_custom_cash_to_config(&path, &[]).unwrap();
        assert!(Config::load_or_default(Some(&path)).unwrap().custom_cash().is_empty());
        let _ = fs::remove_file(path);
    }
}
