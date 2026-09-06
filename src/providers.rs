use crate::models::BalanceItem;
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

        let sym = slug_to_symbol(&slug);

        // 1. First priority: Direct Yahoo Finance query with canonical symbol (fast, reliable ~20ms)
        if let Some(price) = self.fetch_fx_rate(&format!("{sym}-USD")).await {
            return (sym, format_price(price), Some(price));
        }

        // 2. Second priority: CoinMarketCap __NEXT_DATA__
        let url = if target.starts_with("http://") || target.starts_with("https://") {
            target.to_string()
        } else {
            format!("https://coinmarketcap.com/currencies/{slug}/")
        };

        if let Ok(resp) = self.client.get(&url).send().await
            && resp.status().is_success()
            && let Ok(html) = resp.text().await
            && let Some(next_data) = extract_next_data(&html)
            && let Ok(json) = serde_json::from_str::<Value>(&next_data) {
                let detail = &json["props"]["pageProps"]["detailRes"]["detail"];
                let symbol = detail["symbol"].as_str().map(|s| s.to_string()).unwrap_or_else(|| sym.clone());
                let price = detail["statistics"]["price"].as_f64();

                if let Some(p) = price {
                    return (symbol, format_price(p), Some(p));
                }
            }

        // 3. Fallback: CoinGecko simple price API
        let cg_url = format!("https://api.coingecko.com/api/v3/simple/price?ids={slug}&vs_currencies=usd");
        if let Ok(resp) = self.client.get(&cg_url).send().await
            && resp.status().is_success()
            && let Ok(json) = resp.json::<Value>().await
            && let Some(price) = json.get(&slug).and_then(|c| c.get("usd")).and_then(|u| u.as_f64()) {
                return (sym, format_price(price), Some(price));
            }

        (sym, "FAILED".to_string(), None)
    }

    /// Fetch asset price in USD dynamically for any cryptocurrency or stablecoin
    pub async fn fetch_asset_price_usd(&self, symbol: &str) -> Option<f64> {
        let sym_norm = crate::models::normalize_crypto_symbol(symbol);
        if sym_norm == "USD" {
            return Some(1.0);
        }
        self.fetch_fx_rate(&format!("{sym_norm}-USD")).await
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
    pub async fn fetch_uphold_cards(&self, token: &str) -> Option<Vec<(String, f64)>> {
        let cards = self.do_fetch_uphold_cards(token).await;

        if let Some(ref c) = cards
            && !c.is_empty() {
                return cards;
            }

        // If empty or failed (e.g. token expired with 401 or invalid), try extracting from Brave CDP
        if let Some(new_token) = self.try_extract_uphold_token_from_cdp().await
            && let Some(new_cards) = self.do_fetch_uphold_cards(&new_token).await
            && !new_cards.is_empty() {
                save_uphold_token_to_config(&new_token);
                return Some(new_cards);
            }

        cards
    }

    async fn do_fetch_uphold_cards(&self, token: &str) -> Option<Vec<(String, f64)>> {
        if token.is_empty() {
            return None;
        }
        let url = "https://api.uphold.com/v0/me/cards";

        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let json = resp.json::<Value>().await.ok()?;
        let arr = json.as_array()?;

        let mut cards = Vec::new();
        for card in arr {
            let currency = card.get("currency").and_then(|c| c.as_str()).unwrap_or("");
            let balance_str = card.get("balance").and_then(|b| b.as_str()).unwrap_or("0");
            let balance: f64 = balance_str.parse().unwrap_or(0.0);

            if balance > 0.0 && !currency.is_empty() {
                cards.push((currency.to_string(), balance));
            }
        }

        Some(cards)
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
        #[cfg(test)]
        {
            return None;
        }
        #[cfg(not(test))]
        {
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
    }


    /// Fetch Coinbase accounts with positive balance (modern CDP / Advanced Trade integration)
    pub async fn fetch_coinbase_balances(&self, api_key: &str, api_secret: &str) -> Option<Vec<(String, f64, f64)>> {
        use base64::prelude::*;
        use ed25519_dalek::{SigningKey, Signer};
        use std::time::{SystemTime, UNIX_EPOCH};

        if api_key.is_empty() || api_secret.is_empty() {
            return None;
        }

        let mut results = Vec::new();

        // 1. Decode raw secret (base64)
        let raw_bytes = match BASE64_STANDARD.decode(api_secret.trim()) {
            Ok(b) => b,
            Err(_) => return None,
        };

        let seed: [u8; 32] = if raw_bytes.len() >= 32 {
            let mut s = [0u8; 32];
            s.copy_from_slice(&raw_bytes[..32]);
            s
        } else {
            return None;
        };

        let signing_key = SigningKey::from_bytes(&seed);

        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(_) => return None,
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
            Ok(r) if r.status().is_success() => r,
            _ => return None,
        };

        let json: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return None,
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

        Some(results)
    }

    /// Fetch live FX rate from Yahoo Finance (e.g. "USDCHF=X")
    pub async fn fetch_fx_rate(&self, symbol: &str) -> Option<f64> {
        let hosts = ["query1.finance.yahoo.com", "query2.finance.yahoo.com"];
        for host in hosts {
            let url = format!("https://{host}/v8/finance/chart/{symbol}");
            if let Ok(resp) = self.client.get(&url).send().await
                && resp.status().is_success()
                && let Ok(json) = resp.json::<Value>().await
                && let Some(results) = json["chart"]["result"].as_array()
                && let Some(meta) = results.first().and_then(|r| r.get("meta"))
                && let Some(price_val) = meta.get("regularMarketPrice").and_then(|p| p.as_f64()) {
                    return Some(price_val);
                }
        }
        None
    }

    /// Fetch all FX conversion rates to CHF
    pub async fn fetch_fx_rates(&self) -> crate::models::FxRates {
        let mut rates = crate::models::FxRates::default();
        if let Some(r) = self.fetch_fx_rate("USDCHF=X").await {
            rates.usd_to_chf = r;
        }
        if let Some(r) = self.fetch_fx_rate("EURCHF=X").await {
            rates.eur_to_chf = r;
        }
        if let Some(r) = self.fetch_fx_rate("GBPCHF=X").await {
            rates.gbp_to_chf = r;
        }
        if let Some(r) = self.fetch_fx_rate("AUDCHF=X").await {
            rates.aud_to_chf = r;
        }
        rates
    }

    /// Fetch Chia wallet balance by reading local sqlite DB
    pub fn fetch_chia_balance(&self, custom_path: Option<&str>) -> Option<f64> {
        let db_path = if let Some(p) = custom_path {
            std::path::PathBuf::from(p)
        } else {
            let home = std::env::var("HOME").ok()?;
            let dir = std::path::PathBuf::from(home).join(".chia/mainnet/wallet/db");
            let fingerprint_file = dir.join("last_used_fingerprint");
            if let Ok(fp) = std::fs::read_to_string(&fingerprint_file) {
                let clean_fp = fp.trim();
                let candidate = dir.join(format!("blockchain_wallet_v2_r1_mainnet_{clean_fp}.sqlite"));
                if candidate.exists() {
                    candidate
                } else {
                    find_first_chia_db(&dir)?
                }
            } else {
                find_first_chia_db(&dir)?
            }
        };

        if !db_path.exists() {
            return None;
        }

        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        ).ok()?;

        let mut stmt = conn.prepare("SELECT amount FROM coin_record WHERE spent = 0 AND wallet_id = 1").ok()?;
        let rows = stmt.query_map([], |row| {
            let blob: Vec<u8> = row.get(0)?;
            Ok(blob)
        }).ok()?;

        let mut total_mojos: u128 = 0;
        for row in rows.flatten() {
            let mut val: u128 = 0;
            for b in row {
                val = (val << 8) | (b as u128);
            }
            total_mojos += val;
        }

        let total_xch = (total_mojos as f64) / 1_000_000_000_000.0;
        Some(total_xch)
    }

    /// Fetch Starling Bank account balances
    pub async fn fetch_starling_balances(&self, token: &str) -> Option<Vec<(String, f64)>> {
        if token.is_empty() {
            return None;
        }

        let accounts_url = "https://api.starlingbank.com/api/v2/accounts";
        let resp = match self
            .client
            .get(accounts_url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => return None,
        };

        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(_) => return None,
        };

        let accounts = match json.get("accounts").and_then(|a| a.as_array()) {
            Some(arr) => arr,
            None => return None,
        };

        let mut results = Vec::new();
        for acc in accounts {
            let account_uid = match acc.get("accountUid").and_then(|u| u.as_str()) {
                Some(uid) => uid,
                None => continue,
            };

            let balance_url = format!("https://api.starlingbank.com/api/v2/accounts/{account_uid}/balance");
            if let Ok(b_resp) = self
                .client
                .get(&balance_url)
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                && b_resp.status().is_success()
                && let Ok(b_json) = b_resp.json::<Value>().await
            {
                let minor_units = b_json
                    .get("totalEffectiveBalance")
                    .and_then(|eb| eb.get("minorUnits"))
                    .and_then(|m| m.as_f64())
                    .or_else(|| {
                        b_json
                            .get("totalClearedBalance")
                            .and_then(|eb| eb.get("minorUnits"))
                            .and_then(|m| m.as_f64())
                    })
                    .or_else(|| {
                        b_json
                            .get("effectiveBalance")
                            .and_then(|eb| eb.get("minorUnits"))
                            .and_then(|m| m.as_f64())
                    })
                    .unwrap_or(0.0);

                let currency = b_json
                    .get("totalEffectiveBalance")
                    .and_then(|eb| eb.get("currency"))
                    .or_else(|| b_json.get("effectiveBalance").and_then(|eb| eb.get("currency")))
                    .and_then(|c| c.as_str())
                    .or_else(|| acc.get("currency").and_then(|c| c.as_str()))
                    .unwrap_or("GBP")
                    .to_string();

                let balance = minor_units / 100.0;
                if let Some(existing) = results.iter_mut().find(|(c, _)| c == &currency) {
                    existing.1 += balance;
                } else {
                    results.push((currency, balance));
                }
            }
        }

        Some(results)
    }

    /// Fetch Kraken balances via authenticated REST API
    pub async fn fetch_kraken_balances(&self, api_key: &str, api_secret: &str) -> Option<Vec<(String, f64)>> {
        use base64::prelude::*;
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256, Sha512};
        use std::time::SystemTime;

        if api_key.is_empty() || api_secret.is_empty() {
            return None;
        }

        let nonce = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_millis(),
            Err(_) => return None,
        };

        let path = "/0/private/Balance";
        let post_data = format!("nonce={nonce}");

        // SHA256(nonce + post_data)
        let mut sha256 = Sha256::new();
        sha256.update(format!("{nonce}{post_data}").as_bytes());
        let sha256_digest = sha256.finalize();

        // HMAC-SHA512 of (path + sha256_digest) using base64-decoded api_secret
        let secret_bytes = match BASE64_STANDARD.decode(api_secret.trim()) {
            Ok(b) => b,
            Err(_) => return None,
        };

        type HmacSha512 = Hmac<Sha512>;
        let mut mac = match HmacSha512::new_from_slice(&secret_bytes) {
            Ok(m) => m,
            Err(_) => return None,
        };

        mac.update(path.as_bytes());
        mac.update(&sha256_digest);
        let api_sign = BASE64_STANDARD.encode(mac.finalize().into_bytes());

        let url = format!("https://api.kraken.com{path}");
        let resp = match self
            .client
            .post(&url)
            .header("API-Key", api_key.trim())
            .header("API-Sign", api_sign)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => return None,
        };

        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(_) => return None,
        };

        if let Some(err_arr) = json.get("error").and_then(|e| e.as_array())
            && !err_arr.is_empty()
        {
            return None;
        }

        let mut results = Vec::new();
        if let Some(res_map) = json.get("result").and_then(|r| r.as_object()) {
            for (asset, val_val) in res_map {
                let amt: f64 = val_val
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| val_val.as_f64())
                    .unwrap_or(0.0);

                if amt > 0.00000001 {
                    let norm_sym = normalize_kraken_asset(asset);
                    results.push((norm_sym, amt));
                }
            }
        }

        Some(results)
    }

    /// Fetch Interactive Brokers portfolio holdings (Cash and Stocks) via Flex Web Service
    pub async fn fetch_ibkr_holdings(&self, token: &str, query_id: &str) -> Option<Vec<IbkrHolding>> {
        if token.trim().is_empty() || query_id.trim().is_empty() {
            return None;
        }

        let send_url = format!(
            "https://ndcdyn.interactivebrokers.com/Universal/servlet/FlexStatementService.SendRequest?t={}&q={}&v=3",
            token.trim(),
            query_id.trim()
        );

        let send_resp = self.client.get(&send_url).timeout(Duration::from_secs(15)).send().await.ok()?;
        if !send_resp.status().is_success() {
            return None;
        }
        let send_xml = send_resp.text().await.ok()?;
        let ref_code = parse_ibkr_send_request_xml(&send_xml).ok()?;

        let get_url = format!(
            "https://ndcdyn.interactivebrokers.com/Universal/servlet/FlexStatementService.GetStatement?q={}&t={}&v=3",
            ref_code.trim(),
            token.trim()
        );

        // IBKR statement generation can take a moment, retry if code 1019
        for _ in 0..4 {
            let stmt_resp = self.client.get(&get_url).timeout(Duration::from_secs(20)).send().await.ok()?;
            if stmt_resp.status().is_success() {
                let stmt_xml = stmt_resp.text().await.ok()?;
                if stmt_xml.contains("<ErrorCode>1019</ErrorCode>") {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    continue;
                }
                let holdings = parse_ibkr_statement_xml(&stmt_xml);
                return Some(holdings);
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }

        None
    }

    pub async fn validate_finpension_token(&self, token: &str) -> bool {
        if token.trim().is_empty() {
            return false;
        }
        let clean_token = token.trim().trim_start_matches("Bearer ").trim();
        for url in &["https://3a.finpension.ch/api/portfolios", "https://vb.finpension.ch/api/portfolios"] {
            if let Ok(resp) = self
                .client
                .get(*url)
                .header("Authorization", format!("Bearer {clean_token}"))
                .header("x-app-platform", "web")
                .header("Accept", "application/json")
                .timeout(Duration::from_secs(4))
                .send()
                .await
                && resp.status().is_success()
            {
                return true;
            }
        }
        false
    }

    /// Extract fresh Finpension Bearer token from an active browser tab via Chrome DevTools Protocol (CDP)
    pub async fn try_extract_finpension_token_from_cdp(&self) -> Option<String> {
        #[cfg(test)]
        {
            return None;
        }
        #[cfg(not(test))]
        {
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
                if url.contains("finpension.ch") && ty == "page"
                    && let Some(ws) = t.get("webSocketDebuggerUrl").and_then(|w| w.as_str()) {
                        ws_url = Some(ws.to_string());
                        break;
                    }
            }

            let ws_url = ws_url?;
            let (mut ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await.ok()?;

            let net_enable = serde_json::json!({ "id": 1, "method": "Network.enable" }).to_string();
            let _ = ws_stream.send(Message::Text(net_enable.into())).await;

            let rt_enable = serde_json::json!({ "id": 2, "method": "Runtime.enable" }).to_string();
            let _ = ws_stream.send(Message::Text(rt_enable.into())).await;

            // Check storage (including Redux Persist root) for access_token or JWT
            let check_storage_js = r#"
            (() => {
                try {
                    const rootStr = sessionStorage.getItem("persist:root") || localStorage.getItem("persist:root");
                    if (rootStr) {
                        const root = JSON.parse(rootStr);
                        const auth = typeof root.auth === "string" ? JSON.parse(root.auth) : root.auth;
                        if (auth && auth.token) return auth.token;
                    }
                } catch(e) {}
                for (let [k, v] of Object.entries(localStorage).concat(Object.entries(sessionStorage))) {
                    if (typeof v === 'string') {
                        if (v.startsWith('ey') && v.length > 40 && v.includes('.')) return v;
                        try {
                            const p = JSON.parse(v);
                            const t = p?.access_token || p?.accessToken || p?.token || p?.jwt || p?.idToken || p?.state?.token;
                            if (t && typeof t === 'string' && t.length > 20) return t;
                        } catch(e) {}
                    }
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

            // Trigger fetch from page context so native auth/interceptor headers attach
            let trigger_fetch = serde_json::json!({
                "id": 4,
                "method": "Runtime.evaluate",
                "params": {
                    "expression": "try { fetch('https://3a.finpension.ch/api/portfolios', { headers: { 'x-app-platform': 'web', 'Accept': 'application/json' } }); fetch('https://vb.finpension.ch/api/portfolios', { headers: { 'x-app-platform': 'web', 'Accept': 'application/json' } }); } catch(e) {}"
                }
            }).to_string();
            let _ = ws_stream.send(Message::Text(trigger_fetch.into())).await;

            let start = tokio::time::Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                let msg = match tokio::time::timeout(Duration::from_millis(1500), ws_stream.next()).await {
                    Ok(Some(Ok(Message::Text(txt)))) => txt,
                    _ => break,
                };

                if let Ok(val) = serde_json::from_str::<Value>(&msg) {
                    // Storage evaluation response
                    if val.get("id") == Some(&serde_json::json!(3))
                        && let Some(tok) = val.pointer("/result/result/value").and_then(|v| v.as_str())
                            && self.validate_finpension_token(tok).await {
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
                                            if self.validate_finpension_token(candidate).await {
                                                return Some(candidate.to_string());
                                            }
                                        }
                            }
                        }
                }
            }

            None
        }
    }

    /// Fetch Finpension portfolios (3a, Vested Benefits / Freizügigkeit, Invest).
    /// If token is None or expired, attempts automatic CDP extraction from Brave browser.
    /// Returns (portfolios, Option<newly_extracted_token>)
    pub async fn fetch_finpension_portfolios(&self, token_opt: Option<&str>) -> Option<(Vec<(String, f64)>, Option<String>)> {
        if let Some(token) = token_opt
            && !token.trim().is_empty()
            && let Some(portfolios) = self.do_fetch_finpension_portfolios(token).await {
                return Some((portfolios, None));
            }

        // Token missing or expired -> attempt silent CDP extraction from Brave tab
        if let Some(new_token) = self.try_extract_finpension_token_from_cdp().await
            && let Some(portfolios) = self.do_fetch_finpension_portfolios(&new_token).await {
                save_finpension_token_to_config(&new_token);
                return Some((portfolios, Some(new_token)));
            }

        None
    }

    async fn do_fetch_finpension_portfolios(&self, token: &str) -> Option<Vec<(String, f64)>> {
        if token.trim().is_empty() {
            return None;
        }

        let clean_token = token.trim().trim_start_matches("Bearer ").trim();
        let endpoints = [
            "https://3a.finpension.ch/api/portfolios",
            "https://vb.finpension.ch/api/portfolios",
            "https://invest.finpension.ch/api/portfolios",
        ];

        let mut all_portfolios = Vec::new();
        let mut any_success = false;

        for url in endpoints {
            let resp = self
                .client
                .get(url)
                .header("Authorization", format!("Bearer {clean_token}"))
                .header("x-app-platform", "web")
                .header("Accept", "application/json")
                .timeout(Duration::from_secs(10))
                .send()
                .await;

            if let Ok(r) = resp
                && r.status().is_success()
            {
                any_success = true;
                if let Ok(json) = r.json::<Value>().await
                    && let Some(items) = parse_finpension_portfolios_json(&json)
                {
                    all_portfolios.extend(items);
                }
            }
        }

        if any_success && !all_portfolios.is_empty() {
            Some(all_portfolios)
        } else {
            None
        }
    }

    /// Fetch live Swissquote balances (Stocks and Cash) from an active browser session via Chrome DevTools Protocol (CDP)
    pub async fn fetch_swissquote_balances(&self) -> Option<Vec<BalanceItem>> {
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
                if url.contains("swissquote") && ty == "page"
                    && let Some(ws) = t.get("webSocketDebuggerUrl").and_then(|w| w.as_str()) {
                        ws_url = Some(ws.to_string());
                        break;
                    }
            }

            let ws_url = ws_url?;
            let (mut ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await.ok()?;

            let rt_enable = serde_json::json!({ "id": 1, "method": "Runtime.enable" }).to_string();
            let _ = ws_stream.send(Message::Text(rt_enable.into())).await;

            let extract_js = r#"
            (() => {
                const bodyText = document.body ? document.body.innerText : "";
                if (!bodyText.includes("Totalwert") || (!bodyText.includes("Positionen") && !bodyText.includes("Barguthaben"))) {
                    return JSON.stringify({ status: "not_logged_in" });
                }

                const items = [];
                const tables = Array.from(document.querySelectorAll("table.s-table"));
                const currTable = tables.find(t => t.innerText.includes("Währung") && t.innerText.includes("Kontosaldo"));

                let foundCash = false;
                if (currTable) {
                    for (const tr of currTable.querySelectorAll("tbody tr, tr")) {
                        const cells = Array.from(tr.cells || tr.children).map(c => c.innerText.trim());
                        if (cells.length >= 6) {
                            const curr = cells[1];
                            const rawKurs = cells[2].replace(/'/g, "");
                            const rawSaldo = cells[3].replace(/'/g, "");
                            const saldo = parseFloat(rawSaldo);
                            const kurs = parseFloat(rawKurs) || 1.0;
                            if (curr && !isNaN(saldo) && saldo > 0.0001 && curr !== "Gesamt CHF") {
                                const valChf = (curr === "CHF") ? saldo : (saldo * kurs);
                                items.push({
                                    account: "SQ",
                                    category: "Cash",
                                    symbol: curr,
                                    amount: saldo,
                                    native_currency: curr,
                                    value_native: saldo,
                                    value_chf: Math.round(valChf * 100) / 100
                                });
                                foundCash = true;
                            }
                        }
                    }
                }

                if (!foundCash) {
                    const m = bodyText.match(/(?:Barguthaben|Verfügbarer Betrag)\s*([\d\x27\.]+)\s*CHF/);
                    if (m) {
                        const cash = parseFloat(m[1].replace(/'/g, ""));
                        if (!isNaN(cash) && cash > 0) {
                            items.push({
                                account: "SQ",
                                category: "Cash",
                                symbol: "CHF",
                                amount: cash,
                                native_currency: "CHF",
                                value_native: cash,
                                value_chf: cash
                            });
                        }
                    }
                }

                const posTable = tables.find(t => t.innerText.includes("Produkt") && t.innerText.includes("Anzahl") && t.innerText.includes("Totalwert CHF"));
                if (posTable) {
                    for (const tr of posTable.querySelectorAll("tbody tr, tr")) {
                        const cells = Array.from(tr.cells || tr.children).map(c => c.innerText.trim());
                        if (cells.length >= 15 && cells[0] === "BuySell") {
                            const symbol = cells[2];
                            const amount = parseFloat(cells[3].replace(/'/g, ""));
                            const valNative = parseFloat(cells[5].replace(/'/g, ""));
                            const nativeCurr = cells[10] || "CHF";
                            const valChf = parseFloat(cells[14].replace(/'/g, ""));

                            if (symbol && !isNaN(amount) && amount > 0) {
                                items.push({
                                    account: "SQ",
                                    category: "Stocks",
                                    symbol: symbol,
                                    amount: amount,
                                    native_currency: nativeCurr,
                                    value_native: !isNaN(valNative) ? valNative : (valChf || 0),
                                    value_chf: !isNaN(valChf) ? valChf : (valNative || 0)
                                });
                            }
                        }
                    }
                }

                return JSON.stringify({
                    status: "logged_in",
                    items: items
                });
            })()
            "#;

            let eval_msg = serde_json::json!({
                "id": 2,
                "method": "Runtime.evaluate",
                "params": {
                    "expression": extract_js,
                    "returnByValue": true
                }
            }).to_string();

            let _ = ws_stream.send(Message::Text(eval_msg.into())).await;

            let start = tokio::time::Instant::now();
            while start.elapsed() < Duration::from_secs(4) {
                let msg = match tokio::time::timeout(Duration::from_millis(1500), ws_stream.next()).await {
                    Ok(Some(Ok(Message::Text(txt)))) => txt,
                    _ => break,
                };

                if let Ok(val) = serde_json::from_str::<Value>(&msg)
                    && val.get("id") == Some(&serde_json::json!(2))
                {
                    if let Some(val_str) = val.pointer("/result/result/value").and_then(|v| v.as_str()) {
                        return parse_swissquote_json(val_str);
                    }
                    break;
                }
            }

            None
        }
}

pub fn parse_swissquote_json(json_str: &str) -> Option<Vec<BalanceItem>> {
    let res = serde_json::from_str::<Value>(json_str).ok()?;
    if res.get("status").and_then(|s| s.as_str()) != Some("logged_in") {
        return None;
    }
    let items_val = res.get("items")?;
    let items = serde_json::from_value::<Vec<BalanceItem>>(items_val.clone()).ok()?;
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

pub fn parse_finpension_portfolios_json(val: &Value) -> Option<Vec<(String, f64)>> {
    let arr = val
        .as_array()
        .or_else(|| val.get("data").and_then(|d| d.as_array()))?;

    let mut results = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let name = item
            .get("name")
            .and_then(|n| n.as_str())
            .or_else(|| item.pointer("/strategy/name_en").and_then(|n| n.as_str()))
            .or_else(|| item.pointer("/strategy/name_de").and_then(|n| n.as_str()))
            .or_else(|| item.pointer("/strategy/name").and_then(|n| n.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Portfolio {}", i + 1));

        let value = item
            .pointer("/performance/current_value")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .or_else(|| {
                item.get("current_value")
                    .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            })
            .or_else(|| {
                item.get("total_value")
                    .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            })
            .unwrap_or(0.0);

        if value > 0.0 {
            results.push((name, value));
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IbkrHolding {
    pub category: crate::models::AccountCategory,
    pub symbol: String,
    pub amount: f64,
    pub currency: String,
    pub value_native: f64,
}

pub fn parse_ibkr_send_request_xml(xml: &str) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut status = String::new();
    let mut ref_code = String::new();
    let mut err_msg = String::new();
    let mut current_tag = String::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                current_tag = e.name().as_ref().to_string();
            }
            Ok(Event::Text(ref e)) => {
                let txt = e.as_ref().trim().to_string();
                match current_tag.as_str() {
                    "Status" => status = txt,
                    "ReferenceCode" => ref_code = txt,
                    "ErrorMessage" => err_msg = txt,
                    _ => {}
                }
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }

    if status.eq_ignore_ascii_case("Success") && !ref_code.is_empty() {
        Ok(ref_code)
    } else if !err_msg.is_empty() {
        Err(err_msg)
    } else if !status.is_empty() {
        Err(format!("IBKR response status: {status}"))
    } else {
        Err("Invalid IBKR XML response".to_string())
    }
}

pub fn parse_ibkr_statement_xml(xml: &str) -> Vec<IbkrHolding> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut holdings = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let tag = name.as_ref();
                if tag == "OpenPosition" {
                    let mut symbol = String::new();
                    let mut position: f64 = 0.0;
                    let mut mark_price: f64 = 0.0;
                    let mut currency = "USD".to_string();
                    let mut position_value: Option<f64> = None;

                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let val = attr.value.as_ref().to_string();
                        match key {
                            "symbol" => symbol = val,
                            "position" => position = val.parse().unwrap_or(0.0),
                            "markPrice" => mark_price = val.parse().unwrap_or(0.0),
                            "currency" => currency = val.to_uppercase(),
                            "positionValue" => position_value = val.parse().ok(),
                            _ => {}
                        }
                    }

                    if !symbol.is_empty() && position.abs() > 0.000001 {
                        let val_native = position_value.unwrap_or(position * mark_price);
                        holdings.push(IbkrHolding {
                            category: crate::models::AccountCategory::Stocks,
                            symbol,
                            amount: position,
                            currency,
                            value_native: val_native,
                        });
                    }
                } else if tag == "CashReportCurrency" || tag == "CashSummaryItem" || tag == "ChangeInCash" {
                    let mut currency = String::new();
                    let mut ending_cash: f64 = 0.0;

                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let val = attr.value.as_ref().to_string();
                        match key {
                            "currency" => currency = val.to_uppercase(),
                            "endingCash" => ending_cash = val.parse().unwrap_or(0.0),
                            _ => {}
                        }
                    }

                    if !currency.is_empty() && currency != "BASE_SUMMARY" && ending_cash.abs() > 0.0001 {
                        holdings.push(IbkrHolding {
                            category: crate::models::AccountCategory::Cash,
                            symbol: currency.clone(),
                            amount: ending_cash,
                            currency,
                            value_native: ending_cash,
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    holdings
}

fn normalize_kraken_asset(asset: &str) -> String {
    let u = asset.to_uppercase();
    if u.len() == 4 && (u.starts_with('Z') || u.starts_with('X')) {
        let base = &u[1..];
        match base {
            "XBT" => "BTC".to_string(),
            "DG" => "DOGE".to_string(),
            other => other.to_string(),
        }
    } else if u == "XXBT" {
        "BTC".to_string()
    } else if u == "XDG" {
        "DOGE".to_string()
    } else {
        u
    }
}

pub fn slug_to_symbol(slug: &str) -> String {
    match slug.to_lowercase().as_str() {
        "monero" => "XMR".to_string(),
        "bitcoin" => "BTC".to_string(),
        "ethereum" => "ETH".to_string(),
        "basic-attention-token" => "BAT".to_string(),
        "dogecoin" => "DOGE".to_string(),
        "chia" | "chia-network" => "XCH".to_string(),
        "the-graph" => "GRT".to_string(),
        "stellar" => "XLM".to_string(),
        "compound" => "COMP".to_string(),
        "numeraire" => "NMR".to_string(),
        "nucypher" => "NU".to_string(),
        "polygon" => "MATIC".to_string(),
        "skale-network" => "SKL".to_string(),
        "presearch" => "PRE".to_string(),
        "ampleforth" => "AMPL".to_string(),
        "celo" => "CELO".to_string(),
        "uma" => "UMA".to_string(),
        "grin" => "GRIN".to_string(),
        other => other.to_uppercase(),
    }
}



fn find_first_chia_db(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with("blockchain_wallet_v2_r1_") && name.ends_with(".sqlite") {
                    return Some(path);
                }
        }
    }
    None
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
    #[cfg(test)]
    {
        let _ = token;
        return;
    }
    #[cfg(not(test))]
    {
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
}

fn save_finpension_token_to_config(token: &str) {
    #[cfg(test)]
    {
        let _ = token;
        return;
    }
    #[cfg(not(test))]
    {
        for path in &["config.json", "/home/diego/code/dashboard/config.json"] {
            if let Ok(content) = std::fs::read_to_string(path)
                && let Ok(mut val) = serde_json::from_str::<Value>(&content) {
                    val["finpension_token"] = serde_json::json!(token);
                    let tmp_path = format!("{path}.tmp");
                    if let Ok(serialized) = serde_json::to_string_pretty(&val)
                        && std::fs::write(&tmp_path, serialized).is_ok() {
                            let _ = std::fs::rename(tmp_path, path);
                        }
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
        let (btc_sym, btc_str, btc_num) = providers.fetch_crypto("https://coinmarketcap.com/currencies/bitcoin/").await;
        println!("Live BTC: sym={}, str={}, num={:?}", btc_sym, btc_str, btc_num);

        let (xmr_sym, xmr_str, xmr_num) = providers.fetch_crypto("https://coinmarketcap.com/currencies/monero/").await;
        println!("Live XMR: sym={}, str={}, num={:?}", xmr_sym, xmr_str, xmr_num);

        let (eth_sym, eth_str, eth_num) = providers.fetch_crypto("https://coinmarketcap.com/currencies/ethereum/").await;
        println!("Live ETH: sym={}, str={}, num={:?}", eth_sym, eth_str, eth_num);

        assert!(btc_num.is_some());
        assert!(xmr_num.is_some());
        assert!(eth_num.is_some());
    }

    #[test]
    fn test_slug_to_symbol() {
        assert_eq!(slug_to_symbol("monero"), "XMR");
        assert_eq!(slug_to_symbol("ethereum"), "ETH");
        assert_eq!(slug_to_symbol("bitcoin"), "BTC");
        assert_eq!(slug_to_symbol("basic-attention-token"), "BAT");
        assert_eq!(slug_to_symbol("dogecoin"), "DOGE");
        assert_eq!(slug_to_symbol("chia"), "XCH");
        assert_eq!(slug_to_symbol("the-graph"), "GRT");
        assert_eq!(slug_to_symbol("stellar"), "XLM");
        assert_eq!(slug_to_symbol("atom"), "ATOM");
    }

    #[tokio::test]
    async fn test_fetch_asset_price_usd() {
        let providers = Providers::new();
        let usd = providers.fetch_asset_price_usd("USD").await;
        assert_eq!(usd, Some(1.0));

        let usdc = providers.fetch_asset_price_usd("USDC").await;
        assert_eq!(usdc, Some(1.0));

        let xmr = providers.fetch_asset_price_usd("XMR").await;
        assert!(xmr.is_some() && xmr.unwrap() > 0.0);

        let eth = providers.fetch_asset_price_usd("ETH").await;
        assert!(eth.is_some() && eth.unwrap() > 0.0);
    }


    #[tokio::test]
    async fn test_fetch_coinbase_live() {
        let providers = Providers::new();
        if let Ok(cfg_str) = std::fs::read_to_string("config.json") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg_str) {
                if let (Some(key), Some(secret)) = (v.get("coinbase_api_key").and_then(|k| k.as_str()), v.get("coinbase_api_secret").and_then(|s| s.as_str())) {
                    if let Some(balances) = providers.fetch_coinbase_balances(key, secret).await {
                        println!("Fetched {} Coinbase accounts: {:?}", balances.len(), balances);
                        assert!(!balances.is_empty());
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_fetch_fx_rates_live() {
        let providers = Providers::new();
        let rates = providers.fetch_fx_rates().await;
        assert!(rates.usd_to_chf > 0.5 && rates.usd_to_chf < 1.5);
        assert!(rates.eur_to_chf > 0.5 && rates.eur_to_chf < 1.5);
        assert!(rates.gbp_to_chf > 0.5 && rates.gbp_to_chf < 1.8);
    }

    #[test]
    fn test_fetch_chia_balance_live() {
        let providers = Providers::new();
        if let Some(amt) = providers.fetch_chia_balance(None) {
            assert!(amt > 0.25);
            println!("Live Chia wallet balance: {:.8} XCH", amt);
        }
    }

    #[test]
    fn test_normalize_kraken_asset() {
        assert_eq!(normalize_kraken_asset("ZCHF"), "CHF");
        assert_eq!(normalize_kraken_asset("ZUSD"), "USD");
        assert_eq!(normalize_kraken_asset("ZEUR"), "EUR");
        assert_eq!(normalize_kraken_asset("ZGBP"), "GBP");
        assert_eq!(normalize_kraken_asset("XXBT"), "BTC");
        assert_eq!(normalize_kraken_asset("XETH"), "ETH");
        assert_eq!(normalize_kraken_asset("XXMR"), "XMR");
        assert_eq!(normalize_kraken_asset("SOL"), "SOL");
        assert_eq!(normalize_kraken_asset("USDC"), "USDC");
    }

    #[tokio::test]
    async fn test_fetch_starling_empty_token() {
        let providers = Providers::new();
        let res = providers.fetch_starling_balances("").await;
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn test_fetch_kraken_empty_credentials() {
        let providers = Providers::new();
        let res = providers.fetch_kraken_balances("", "").await;
        assert!(res.is_none());
    }

    #[test]
    fn test_parse_ibkr_send_request_xml_success() {
        let xml = r#"<FlexStatementResponse timestamp="05 September, 2026">
            <Status>Success</Status>
            <ReferenceCode>9876543210</ReferenceCode>
            <Url>https://ndcdyn.interactivebrokers.com/Universal/servlet/FlexStatementService.GetStatement</Url>
        </FlexStatementResponse>"#;
        let res = parse_ibkr_send_request_xml(xml);
        assert_eq!(res, Ok("9876543210".to_string()));
    }

    #[test]
    fn test_parse_ibkr_send_request_xml_error() {
        let xml = r#"<FlexStatementResponse timestamp="05 September, 2026">
            <Status>Warn</Status>
            <ErrorCode>1018</ErrorCode>
            <ErrorMessage>Token has expired or is invalid</ErrorMessage>
        </FlexStatementResponse>"#;
        let res = parse_ibkr_send_request_xml(xml);
        assert_eq!(res, Err("Token has expired or is invalid".to_string()));
    }

    #[test]
    fn test_parse_ibkr_statement_xml() {
        let xml = r#"<FlexQueryResponse queryName="Portfolio" type="AF">
            <FlexStatements count="1">
                <FlexStatement accountId="U1234567">
                    <OpenPositions>
                        <OpenPosition accountId="U1234567" currency="USD" symbol="AAPL" position="10" markPrice="175.50" positionValue="1755.00" />
                        <OpenPosition accountId="U1234567" currency="CHF" symbol="NESN" position="20" markPrice="98.20" positionValue="1964.00" />
                    </OpenPositions>
                    <CashReport>
                        <CashReportCurrency accountId="U1234567" currency="USD" endingCash="2500.50" />
                        <CashReportCurrency accountId="U1234567" currency="CHF" endingCash="850.00" />
                        <CashReportCurrency accountId="U1234567" currency="EUR" endingCash="0.00" />
                    </CashReport>
                </FlexStatement>
            </FlexStatements>
        </FlexQueryResponse>"#;

        let holdings = parse_ibkr_statement_xml(xml);
        assert_eq!(holdings.len(), 4);
        assert_eq!(holdings[0].category, crate::models::AccountCategory::Stocks);
        assert_eq!(holdings[0].symbol, "AAPL");
        assert_eq!(holdings[0].amount, 10.0);
        assert_eq!(holdings[0].currency, "USD");
        assert_eq!(holdings[0].value_native, 1755.0);

        assert_eq!(holdings[1].category, crate::models::AccountCategory::Stocks);
        assert_eq!(holdings[1].symbol, "NESN");
        assert_eq!(holdings[1].amount, 20.0);
        assert_eq!(holdings[1].currency, "CHF");
        assert_eq!(holdings[1].value_native, 1964.0);

        assert_eq!(holdings[2].category, crate::models::AccountCategory::Cash);
        assert_eq!(holdings[2].symbol, "USD");
        assert_eq!(holdings[2].amount, 2500.50);

        assert_eq!(holdings[3].category, crate::models::AccountCategory::Cash);
        assert_eq!(holdings[3].symbol, "CHF");
        assert_eq!(holdings[3].amount, 850.00);
    }

    #[tokio::test]
    async fn test_fetch_ibkr_empty_credentials() {
        let providers = Providers::new();
        let res = providers.fetch_ibkr_holdings("", "").await;
        assert!(res.is_none());
    }

    #[test]
    fn test_parse_finpension_portfolios_json() {
        let json = serde_json::json!([
            {
                "id": 12345,
                "name": "Custom Equity 99",
                "performance": {
                    "current_value": 35420.75,
                    "current_profit_value": 4120.50
                }
            },
            {
                "id": 12346,
                "strategy": {
                    "name": "Finpension Equity 100"
                },
                "current_value": 18250.00
            }
        ]);

        let res = parse_finpension_portfolios_json(&json).unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].0, "Custom Equity 99");
        assert_eq!(res[0].1, 35420.75);
        assert_eq!(res[1].0, "Finpension Equity 100");
        assert_eq!(res[1].1, 18250.00);
    }

    #[tokio::test]
    async fn test_fetch_finpension_empty_token() {
        let providers = Providers::new();
        let res = providers.fetch_finpension_portfolios(Some("")).await;
        assert!(res.is_none());
        let res_none = providers.fetch_finpension_portfolios(None).await;
        assert!(res_none.is_none());
    }

    #[tokio::test]
    async fn test_validate_tokens_empty() {
        let providers = Providers::new();
        assert!(!providers.validate_uphold_token("").await);
        assert!(!providers.validate_finpension_token("").await);
    }

    #[tokio::test]
    async fn test_fetch_starling_live() {
        let providers = Providers::new();
        if let Ok(cfg_str) = std::fs::read_to_string("config.json")
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg_str)
            && let Some(tok) = v.get("starling_token").and_then(|t| t.as_str()) {
                if let Some(balances) = providers.fetch_starling_balances(tok).await {
                    println!("Live Starling balances: {:?}", balances);
                    assert!(!balances.is_empty());
                } else {
                    println!("Starling API rate limited or unavailable; safely handled as None");
                }
            }
    }

    #[test]
    fn test_parse_swissquote_json_valid() {
        let sample = r#"{
            "status": "logged_in",
            "items": [
                {
                    "account": "SQ",
                    "category": "Cash",
                    "symbol": "CHF",
                    "amount": 250.0,
                    "native_currency": "CHF",
                    "value_native": 250.0,
                    "value_chf": 250.0
                },
                {
                    "account": "SQ",
                    "category": "Stocks",
                    "symbol": "AMRZ",
                    "amount": 40.0,
                    "native_currency": "CHF",
                    "value_native": 1480.00,
                    "value_chf": 1480.00
                }
            ]
        }"#;
        let parsed = parse_swissquote_json(sample);
        assert!(parsed.is_some());
        let items = parsed.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].account, "SQ");
        assert_eq!(items[0].category, crate::models::AccountCategory::Cash);
        assert_eq!(items[0].symbol, "CHF");
        assert_eq!(items[0].amount, 250.0);
        assert_eq!(items[1].account, "SQ");
        assert_eq!(items[1].category, crate::models::AccountCategory::Stocks);
        assert_eq!(items[1].symbol, "AMRZ");
        assert_eq!(items[1].value_chf, 1480.00);
    }

    #[test]
    fn test_parse_swissquote_json_not_logged_in_or_empty() {
        assert!(parse_swissquote_json(r#"{"status": "not_logged_in"}"#).is_none());
        assert!(parse_swissquote_json(r#"{"status": "logged_in", "items": []}"#).is_none());
        assert!(parse_swissquote_json("invalid json").is_none());
    }

    #[tokio::test]
    async fn test_fetch_swissquote_live() {
        let providers = Providers::new();
        if let Some(items) = providers.fetch_swissquote_balances().await {
            println!("Fetched Swissquote live items: {}", items.len());
            assert!(!items.is_empty());
        } else {
            println!("No active Swissquote tab or not logged in; safely handled as None");
        }
    }
}




