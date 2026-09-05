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
                .next_back()
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
                        if let Ok(json) = resp.json::<Value>().await
                            && let Some(results) = json["chart"]["result"].as_array()
                                && let Some(meta) = results.first().and_then(|r| r.get("meta"))
                                    && let Some(price_val) = meta.get("regularMarketPrice").and_then(|p| p.as_f64()) {
                                        let symbol = meta
                                            .get("symbol")
                                            .and_then(|s| s.as_str())
                                            .unwrap_or(&ticker)
                                            .to_string();
                                        return (symbol, format_price(price_val), Some(price_val));
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
                .next_back()
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
        if let Ok(resp) = self.client.get(&url).send().await
            && resp.status().is_success()
                && let Ok(html) = resp.text().await
                    && let Some(next_data) = extract_next_data(&html)
                        && let Ok(json) = serde_json::from_str::<Value>(&next_data) {
                            let detail = &json["props"]["pageProps"]["detailRes"]["detail"];
                            let symbol = detail["symbol"].as_str().map(|s| s.to_string());
                            let price = detail["statistics"]["price"].as_f64();

                            if let (Some(sym), Some(p)) = (symbol, price) {
                                return (sym, format_price(p), Some(p));
                            }
                        }

        // 2. Fallback: CoinGecko simple price API
        let cg_url = format!("https://api.coingecko.com/api/v3/simple/price?ids={slug}&vs_currencies=usd");
        if let Ok(resp) = self.client.get(&cg_url).send().await
            && resp.status().is_success()
                && let Ok(json) = resp.json::<Value>().await
                    && let Some(price) = json.get(&slug).and_then(|c| c.get("usd")).and_then(|u| u.as_f64()) {
                        return (slug.to_uppercase(), format_price(price), Some(price));
                    }

        // 3. Fallback: Yahoo Finance crypto chart
        let yf_url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{}-USD", slug.to_uppercase());
        if let Ok(resp) = self.client.get(&yf_url).send().await
            && resp.status().is_success()
                && let Ok(json) = resp.json::<Value>().await
                    && let Some(results) = json["chart"]["result"].as_array()
                        && let Some(price) = results.first().and_then(|r| r["meta"]["regularMarketPrice"].as_f64()) {
                            return (slug.to_uppercase(), format_price(price), Some(price));
                        }

        (slug.to_uppercase(), "FAILED".to_string(), None)
    }

    /// Fetch Monero balance: MoneroOcean pool pending due + local RPC daemon
    pub async fn fetch_monero_balance(&self, address: &str) -> Option<f64> {
        let mut total = 0.0;

        // 1. MoneroOcean pending rewards
        let mo_url = format!("https://api.moneroocean.stream/miner/{address}/stats");
        if let Ok(resp) = self.client.get(&mo_url).send().await
            && let Ok(json) = resp.json::<Value>().await
                && let Some(amt_due) = json.get("amtDue").and_then(|a| a.as_f64()) {
                    total += amt_due / 1_000_000_000_000.0;
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
            && let Ok(json) = resp.json::<Value>().await
                && let Some(balance) = json["result"]["balance"].as_f64() {
                    total += balance / 1_000_000_000_000.0;
                }

        Some(total)
    }

    /// Fetch Uphold cards with positive balance (with automatic CDP token extraction failover)
    pub async fn fetch_uphold_cards(&self, token: &str) -> Vec<(String, f64)> {
        let mut cards = self.do_fetch_uphold_cards(token).await;

        // If empty (e.g. token expired with 401 or invalid), try extracting from Brave CDP
        if cards.is_empty()
            && let Some(new_token) = self.try_extract_uphold_token_from_cdp().await {
                cards = self.do_fetch_uphold_cards(&new_token).await;
                if !cards.is_empty() {
                    save_uphold_token_to_config(&new_token);
                }
            }

        cards
    }

    async fn do_fetch_uphold_cards(&self, token: &str) -> Vec<(String, f64)> {
        let mut cards = Vec::new();
        if token.is_empty() {
            return cards;
        }
        let url = "https://api.uphold.com/v0/me/cards";

        if let Ok(resp) = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            && resp.status().is_success()
                && let Ok(json) = resp.json::<Value>().await
                    && let Some(arr) = json.as_array() {
                        for card in arr {
                            let currency = card.get("currency").and_then(|c| c.as_str()).unwrap_or("");
                            let balance_str = card.get("balance").and_then(|b| b.as_str()).unwrap_or("0");
                            let balance: f64 = balance_str.parse().unwrap_or(0.0);

                            if balance > 0.0 && !currency.is_empty() {
                                cards.push((currency.to_string(), balance));
                            }
                        }
                    }

        cards
    }

    pub async fn validate_uphold_token(&self, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        match self
            .client
            .get("https://api.uphold.com/v0/me/cards")
            .header("Authorization", format!("Bearer {token}"))
            .timeout(Duration::from_secs(4))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Extract fresh Uphold Bearer token from a running Brave instance via Chrome DevTools Protocol (CDP)
    pub async fn try_extract_uphold_token_from_cdp(&self) -> Option<String> {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let targets_resp = self
            .client
            .get("http://127.0.0.1:9222/json")
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .ok()?;
        let targets: Value = targets_resp.json().await.ok()?;
        let targets_arr = targets.as_array()?;

        let mut ws_url = None;
        for t in targets_arr {
            let url = t.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let ty = t.get("type").and_then(|ty| ty.as_str()).unwrap_or("");
            if url.contains("uphold.com") && ty == "page"
                && let Some(ws) = t.get("webSocketDebuggerUrl").and_then(|w| w.as_str()) {
                    ws_url = Some(ws.to_string());
                    break;
                }
        }

        if ws_url.is_none() {
            // Try opening Uphold in a new tab via CDP
            if let Ok(new_resp) = self
                .client
                .put("http://127.0.0.1:9222/json/new?https://wallet.uphold.com/dashboard")
                .timeout(Duration::from_secs(3))
                .send()
                .await
                && let Ok(new_tab) = new_resp.json::<Value>().await
                    && let Some(ws) = new_tab.get("webSocketDebuggerUrl").and_then(|w| w.as_str()) {
                        ws_url = Some(ws.to_string());
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                    }
        }

        let ws_url = ws_url?;
        let (mut ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await.ok()?;

        let net_enable = serde_json::json!({ "id": 1, "method": "Network.enable" }).to_string();
        let _ = ws_stream.send(Message::Text(net_enable.into())).await;

        let rt_enable = serde_json::json!({ "id": 2, "method": "Runtime.enable" }).to_string();
        let _ = ws_stream.send(Message::Text(rt_enable.into())).await;

        // Check storage for access_token
        let check_storage_js = r#"
        (() => {
            for (let [k, v] of Object.entries(localStorage).concat(Object.entries(sessionStorage))) {
                try {
                    const p = JSON.parse(v);
                    const t = p?.access_token || p?.accessToken || p?.token;
                    if (t && typeof t === 'string' && t.length > 20) return t;
                } catch(e) {}
            }
            return null;
        })()
        "#;
        let eval_storage = serde_json::json!({
            "id": 3,
            "method": "Runtime.evaluate",
            "params": {
                "expression": check_storage_js,
                "returnByValue": true
            }
        }).to_string();
        let _ = ws_stream.send(Message::Text(eval_storage.into())).await;

        // Trigger fetch from page context
        let trigger_fetch = serde_json::json!({
            "id": 4,
            "method": "Runtime.evaluate",
            "params": {
                "expression": "try { fetch('https://api.uphold.com/v0/me/cards'); } catch(e) {}"
            }
        }).to_string();
        let _ = ws_stream.send(Message::Text(trigger_fetch.into())).await;

        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_secs(6) {
            let msg = match tokio::time::timeout(Duration::from_millis(1500), ws_stream.next()).await {
                Ok(Some(Ok(Message::Text(txt)))) => txt,
                _ => {
                    let reload = serde_json::json!({ "id": 5, "method": "Page.reload" }).to_string();
                    let _ = ws_stream.send(Message::Text(reload.into())).await;
                    continue;
                }
            };

            if let Ok(val) = serde_json::from_str::<Value>(&msg) {
                // Storage evaluation response
                if val.get("id") == Some(&serde_json::json!(3))
                    && let Some(tok) = val.pointer("/result/result/value").and_then(|v| v.as_str())
                        && self.validate_uphold_token(tok).await {
                            return Some(tok.to_string());
                        }

                // Network request intercept
                if val.get("method") == Some(&serde_json::json!("Network.requestWillBeSent"))
                    && let Some(headers) = val.pointer("/params/request/headers").and_then(|h| h.as_object()) {
                        for (k, v) in headers {
                            if k.eq_ignore_ascii_case("authorization")
                                && let Some(auth_val) = v.as_str()
                                    && auth_val.starts_with("Bearer ") {
                                        let candidate = auth_val.trim_start_matches("Bearer ").trim();
                                        if self.validate_uphold_token(candidate).await {
                                            return Some(candidate.to_string());
                                        }
                                    }
                        }
                    }
            }
        }

        None
    }


    /// Fetch Coinbase accounts with positive balance (modern CDP / Advanced Trade integration)
    pub async fn fetch_coinbase_balances(&self, api_key: &str, api_secret: &str) -> Vec<(String, f64, f64)> {
        use base64::prelude::*;
        use ed25519_dalek::{SigningKey, Signer};
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut results = Vec::new();

        // 1. Decode raw secret (base64)
        let raw_bytes = match BASE64_STANDARD.decode(api_secret.trim()) {
            Ok(b) => b,
            Err(_) => return results,
        };

        let seed: [u8; 32] = if raw_bytes.len() >= 32 {
            let mut s = [0u8; 32];
            s.copy_from_slice(&raw_bytes[..32]);
            s
        } else {
            return results;
        };

        let signing_key = SigningKey::from_bytes(&seed);

        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(_) => return results,
        };
        let exp = now + 120;
        let nonce = format!("{:x}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());

        let header = serde_json::json!({
            "alg": "EdDSA",
            "kid": api_key,
            "nonce": nonce,
            "typ": "JWT"
        });

        let payload = serde_json::json!({
            "iss": "coinbase-cloud",
            "sub": api_key,
            "nbf": now,
            "exp": exp,
            "uri": "GET api.coinbase.com/api/v3/brokerage/accounts"
        });

        let header_b64 = BASE64_URL_SAFE_NO_PAD.encode(header.to_string().as_bytes());
        let payload_b64 = BASE64_URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let signing_input = format!("{}.{}", header_b64, payload_b64);

        let signature = signing_key.sign(signing_input.as_bytes());
        let sig_b64 = BASE64_URL_SAFE_NO_PAD.encode(signature.to_bytes());
        let jwt_token = format!("{}.{}", signing_input, sig_b64);

        let resp = match self
            .client
            .get("https://api.coinbase.com/api/v3/brokerage/accounts")
            .header("Authorization", format!("Bearer {}", jwt_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return results,
        };

        if !resp.status().is_success() {
            return results;
        }

        let json: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return results,
        };

        if let Some(accounts) = json.get("accounts").and_then(|a| a.as_array()) {
            for acc in accounts {
                let currency = acc.get("currency").and_then(|c| c.as_str()).unwrap_or("");
                let avail: f64 = acc
                    .get("available_balance")
                    .and_then(|b| b.get("value"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let hold: f64 = acc
                    .get("hold")
                    .and_then(|b| b.get("value"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let total = avail + hold;
                if total > 0.0 && !currency.is_empty() {
                    results.push((currency.to_string(), total, 0.0));
                }
            }
        }

        results
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
    if price >= 1.0 {
        format!("{:.2}", price)
    } else if price >= 0.001 {
        format!("{:.4}", price)
    } else {
        format!("{:.6}", price)
    }
}

fn save_uphold_token_to_config(token: &str) {
    for path in &["config.json", "/home/diego/code/dashboard/config.json"] {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(mut val) = serde_json::from_str::<Value>(&content) {
                val["uphold_token"] = serde_json::json!(token);
                let tmp_path = format!("{path}.tmp");
                if let Ok(serialized) = serde_json::to_string_pretty(&val)
                    && std::fs::write(&tmp_path, serialized).is_ok() {
                        let _ = std::fs::rename(tmp_path, path);
                    }
            }
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

    #[tokio::test]
    async fn test_fetch_coinbase_live() {
        let providers = Providers::new();
        if let Ok(cfg_str) = std::fs::read_to_string("config.json") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg_str) {
                if let (Some(key), Some(secret)) = (v.get("coinbase_api_key").and_then(|k| k.as_str()), v.get("coinbase_api_secret").and_then(|s| s.as_str())) {
                    let balances = providers.fetch_coinbase_balances(key, secret).await;
                    println!("Fetched {} Coinbase accounts", balances.len());
                    assert!(!balances.is_empty());
                }
            }
        }
    }
}

