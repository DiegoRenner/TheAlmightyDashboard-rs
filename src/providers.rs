use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

pub struct Providers {
    client: Client,
}

impl Providers {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    /// Fetch stock price from Yahoo Finance Chart API (with dual-host failover and delisted handling)
    pub async fn fetch_stock(&self, target: &str) -> (String, String, Option<f64>) {
        let ticker = if target.contains('/') {
            target
                .trim_end_matches('/')
                .split('/')
                .last()
                .unwrap_or(target)
                .to_uppercase()
        } else {
            target.trim().to_uppercase()
        };

        let mut candidates = Vec::new();
        if ticker.ends_with("USD") && !ticker.contains('-') && ticker.len() > 3 {
            let base = &ticker[..ticker.len() - 3];
            candidates.push(format!("{base}-USD"));
        }
        candidates.push(ticker.clone());

        let hosts = ["query1.finance.yahoo.com", "query2.finance.yahoo.com"];

        for sym in candidates {
            for host in hosts {
                let url = format!("https://{host}/v8/finance/chart/{sym}");
                if let Ok(resp) = self.client.get(&url).send().await {
                    let status = resp.status();
                    if status.is_success() {
                        if let Ok(json) = resp.json::<Value>().await {
                            if let Some(results) = json["chart"]["result"].as_array() {
                                if let Some(meta) = results.first().and_then(|r| r.get("meta")) {
                                    if let Some(price_val) = meta.get("regularMarketPrice").and_then(|p| p.as_f64()) {
                                        let symbol = meta
                                            .get("symbol")
                                            .and_then(|s| s.as_str())
                                            .unwrap_or(&ticker)
                                            .to_string();
                                        return (symbol, format_price(price_val), Some(price_val));
                                    }
                                }
                            }
                        }
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        return (ticker, "DELISTED".to_string(), None);
                    }
                }
            }
        }

        (ticker, "FAILED".to_string(), None)
    }

    /// Fetch crypto price from CoinMarketCap (__NEXT_DATA__) with CoinGecko and Yahoo Finance fallbacks
    pub async fn fetch_crypto(&self, target: &str) -> (String, String, Option<f64>) {
        let slug = if target.contains('/') {
            target
                .trim_end_matches('/')
                .split('/')
                .last()
                .unwrap_or(target)
                .to_lowercase()
        } else {
            target.trim().to_lowercase()
        };

        let url = if target.starts_with("http://") || target.starts_with("https://") {
            target.to_string()
        } else {
            format!("https://coinmarketcap.com/currencies/{slug}/")
        };

        // 1. Try CoinMarketCap __NEXT_DATA__
        if let Ok(resp) = self.client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(html) = resp.text().await {
                    if let Some(next_data) = extract_next_data(&html) {
                        if let Ok(json) = serde_json::from_str::<Value>(&next_data) {
                            let detail = &json["props"]["pageProps"]["detailRes"]["detail"];
                            let symbol = detail["symbol"].as_str().map(|s| s.to_string());
                            let price = detail["statistics"]["price"].as_f64();

                            if let (Some(sym), Some(p)) = (symbol, price) {
                                return (sym, format_price(p), Some(p));
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback: CoinGecko simple price API
        let cg_url = format!("https://api.coingecko.com/api/v3/simple/price?ids={slug}&vs_currencies=usd");
        if let Ok(resp) = self.client.get(&cg_url).send().await {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<Value>().await {
                    if let Some(price) = json.get(&slug).and_then(|c| c.get("usd")).and_then(|u| u.as_f64()) {
                        return (slug.to_uppercase(), format_price(price), Some(price));
                    }
                }
            }
        }

        // 3. Fallback: Yahoo Finance crypto chart
        let yf_url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{}-USD", slug.to_uppercase());
        if let Ok(resp) = self.client.get(&yf_url).send().await {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<Value>().await {
                    if let Some(results) = json["chart"]["result"].as_array() {
                        if let Some(price) = results.first().and_then(|r| r["meta"]["regularMarketPrice"].as_f64()) {
                            return (slug.to_uppercase(), format_price(price), Some(price));
                        }
                    }
                }
            }
        }

        (slug.to_uppercase(), "FAILED".to_string(), None)
    }

    /// Fetch Monero balance: MoneroOcean pool pending due + local RPC daemon
    pub async fn fetch_monero_balance(&self, address: &str) -> Option<f64> {
        let mut total = 0.0;

        // 1. MoneroOcean pending rewards
        let mo_url = format!("https://api.moneroocean.stream/miner/{address}/stats");
        if let Ok(resp) = self.client.get(&mo_url).send().await {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(amt_due) = json.get("amtDue").and_then(|a| a.as_f64()) {
                    total += amt_due / 1_000_000_000_000.0;
                }
            }
        }

        // 2. Local Monero wallet RPC (port 28088)
        let rpc_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "0",
            "method": "get_balance"
        });
        if let Ok(resp) = self
            .client
            .post("http://127.0.0.1:28088/json_rpc")
            .json(&rpc_body)
            .send()
            .await
        {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(balance) = json["result"]["balance"].as_f64() {
                    total += balance / 1_000_000_000_000.0;
                }
            }
        }

        Some(total)
    }

    /// Fetch Uphold cards with positive balance
    pub async fn fetch_uphold_cards(&self, token: &str) -> Vec<(String, f64)> {
        let mut cards = Vec::new();
        let url = "https://api.uphold.com/v0/me/cards";

        if let Ok(resp) = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(arr) = json.as_array() {
                    for card in arr {
                        let currency = card.get("currency").and_then(|c| c.as_str()).unwrap_or("");
                        let balance_str = card.get("balance").and_then(|b| b.as_str()).unwrap_or("0");
                        let balance: f64 = balance_str.parse().unwrap_or(0.0);

                        if balance > 0.0 && !currency.is_empty() {
                            cards.push((currency.to_string(), balance));
                        }
                    }
                }
            }
        }

        cards
    }

    /// Fetch Coinbase accounts with positive balance (public spot / balances)
    pub async fn fetch_coinbase_balances(&self, _api_key: &str, _api_secret: &str) -> Vec<(String, f64, f64)> {
        // Placeholder for modern CDP / Advanced Trade integration
        Vec::new()
    }
}

fn extract_next_data(html: &str) -> Option<String> {
    let marker = "id=\"__NEXT_DATA__\"";
    let pos = html.find(marker)?;
    let tag_end = html[pos..].find('>')? + pos + 1;
    let end = html[tag_end..].find("</script>")? + tag_end;
    Some(html[tag_end..end].trim().to_string())
}

fn format_price(price: f64) -> String {
    if price >= 1000.0 {
        format!("{:.2}", price)
    } else if price >= 1.0 {
        format!("{:.2}", price)
    } else if price >= 0.001 {
        format!("{:.4}", price)
    } else {
        format!("{:.6}", price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_price() {
        assert_eq!(format_price(79850.123), "79850.12");
        assert_eq!(format_price(19.5), "19.50");
        assert_eq!(format_price(0.0705), "0.0705");
        assert_eq!(format_price(0.000179), "0.000179");
    }

    #[test]
    fn test_extract_next_data() {
        let sample_html = r#"
            <html>
                <head>
                    <script id="__NEXT_DATA__" type="application/json">{"props":{"sample":123}}</script>
                </head>
            </html>
        "#;
        let data = extract_next_data(sample_html);
        assert_eq!(data, Some(r#"{"props":{"sample":123}}"#.to_string()));
    }

    #[tokio::test]
    async fn test_fetch_stock_live() {
        let providers = Providers::new();
        let (symbol, price_str, price_num) = providers.fetch_stock("GME").await;
        assert_eq!(symbol, "GME");
        assert_ne!(price_str, "FAILED");
        assert!(price_num.is_some());
    }

    #[tokio::test]
    async fn test_fetch_crypto_live() {
        let providers = Providers::new();
        let (symbol, price_str, price_num) = providers.fetch_crypto("https://coinmarketcap.com/currencies/bitcoin/").await;
        assert_eq!(symbol, "BTC");
        assert_ne!(price_str, "FAILED");
        assert!(price_num.is_some());
    }
}
