use serde::Deserialize;
use std::fs;
use std::path::Path;

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

    #[serde(default = "default_update_freq")]
    pub update_frequency_secs: f64,

}

fn default_update_freq() -> f64 {
    5.0
}

impl Config {
    pub fn load_or_default<P: AsRef<Path>>(path_opt: Option<P>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = if let Some(ref p) = path_opt {
            p.as_ref()
        } else if Path::new("config.json").exists() {
            Path::new("config.json")
        } else if Path::new("config_no_accounts.json").exists() {
            Path::new("config_no_accounts.json")
        } else {
            return Ok(Config::default());
        };

        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
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
}
