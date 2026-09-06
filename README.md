# TheAlmightyDashboard (Rust Edition)

A high-performance, asynchronous terminal dashboard for real-time stock and cryptocurrency quotes, along with personal holdings and portfolio valuations.

Built with **Rust**, **Tokio**, and **Ratatui**.

---

## Features

* **Real-time Quotes (Left Panel)**:
  * **Stocks**: Queries Yahoo Finance Chart API with dual-host failover (`query1`/`query2`). Extracts symbols from MarketWatch URLs, Yahoo Finance URLs, or raw tickers. Gracefully detects and flags delisted stocks (`DELISTED`).
  * **Crypto**: High-speed extraction from CoinMarketCap with fallback to CoinGecko and Yahoo Finance crypto pairs.
  * **Live Delay Timer**: Measures millisecond latency since each ticker's last update.
* **Portfolio & Holdings (Right Panel)**:
  * **11 Connected Financial Providers (100% Dynamic Automation)**:
    * **Interactive Brokers (IB)**: Automated Flex Query statement parsing for 30+ equities, funds, and multi-currency cash balances.
    * **Swissquote (SQ)**: Live Chrome DevTools Protocol (CDP) session feed for Swiss/US/European equities and multi-currency cash (`CHF`, `USD`, `AUD`).
    * **Finpension (FP)**: Live CDP session feed tracking 3a and vested benefits retirement portfolios.
    * **UBS**: Live CDP session feed extracting Privatkonto, Sparkonto, and Prepaid Mastercard cash balances.
    * **Revolut (REV)**: Live CDP session feed inspecting multi-currency accounts and pockets (`GBP`, `CHF`, `EUR`, `USD`).
    * **Starling Bank (ST)**: Direct REST API integration via Personal Access Token (`GBP` cash).
    * **Coinbase (CB)**: Authenticated Ed25519 JWT cloud API for all crypto asset wallets.
    * **Kraken**: Authenticated HMAC-SHA512 cloud API for crypto holdings (`BTC`, `ETH`).
    * **Monero (MW)**: Pending mining pool rewards (MoneroOcean) + live Monero daemon wallet RPC.
    * **Chia (CW)**: Local `chia` wallet CLI / SQLite integration for farmer balances.
    * **Uphold (UH)**: Live CDP session feed for positive multi-asset cards.
  * **Base Currency**: Unified CHF base currency valuation with live Yahoo Finance FX conversion (`USD/CHF`, `EUR/CHF`, `GBP/CHF`, `AUD/CHF`).
  * **Offline Disk Cache**: High-reliability fallback caching (`~/.cache/the-almighty-dashboard/balances.json`) ensuring balances and total net worth are preserved when offline or logged out.

---

## Account Marking & Color Coding Legend

### 1. Account Tags (`Acc` Column)
* **No Asterisk** (e.g. `IB`, `CB`, `Kraken`, `ST`, `MW`, `CW`):
  * **Direct Cloud API or Local Daemon**: Operates independently in the background via authenticated API keys or local daemons without requiring an open browser tab.
* **With Asterisk `*`** (e.g. `SQ*`, `FP*`, `UBS*`, `REV*`, `UH*`):
  * **Browser Session Feed**: Dynamically extracted from an active Brave browser tab via Chrome DevTools Protocol (CDP, port `9222`).

### 2. Account Status Colors (`Acc` Column)
* **Yellow (Bold)**: Direct API / Local Daemon is connected and updated live.
* **Light Cyan (Bold)**: Browser Session Feed is active, logged in, and updating live.
* **Light Red (Bold)**: Account is **Stale** (browser tab closed, session expired/logged out, or API temporarily unreachable). The dashboard safely retains the last known balance from disk cache so total net worth remains accurate.

### 3. Asset Category Colors (`Cat` Column)
* **Magenta**: `Crypto` (Bitcoin, Ethereum, Monero, Chia, etc.)
* **Blue**: `Stocks` (Equities, ETFs, mutual funds)
* **Cyan**: `Cash` (Checking accounts, savings accounts, prepaid cards, and fiat cash in `CHF`, `USD`, `EUR`, `GBP`, `AUD`)
* **Green**: `Retirement` (Tax-advantaged retirement portfolios, e.g. Finpension 3a / Vested Benefits)

### 4. Asset Holdings & Valuation
* **Asset / Symbol**: Bold White (Ticker, currency code, or account description).
* **Amount**: White (Dynamic precision: 4 decimals for standard balances, 8 decimals for fractional crypto).
* **Val (CHF)**: Bold Green formatted with Swiss thousand-apostrophe notation (e.g. `26'564.82`).

---

## Keyboard Navigation

* `q` / `Q`: Exit cleanly (restores cursor and terminal state)
* `j` / `Down Arrow`: Scroll down holdings table
* `k` / `Up Arrow`: Scroll up holdings table

---

## Usage

### Run with Default Config (`config_no_accounts.json`)
```bash
cargo run --release
```

### Run with Custom Config / Accounts
```bash
cargo run --release -- path/to/config.json
```

Or run the compiled binary directly:
```bash
./target/release/the-almighty-dashboard config.json
```

---

## Configuration

The dashboard uses `config.json` (see `config_template.json` for all available fields):

```json
{
  "monero_wallet_address": "<optional_address>",
  "coinbase_api_key": "<optional_coinbase_key>",
  "coinbase_api_secret": "<optional_coinbase_secret>",
  "starling_token": "<optional_starling_token>",
  "kraken_api_key": "<optional_kraken_key>",
  "kraken_api_secret": "<optional_kraken_secret>",
  "ibkr_flex_token": "<optional_ibkr_flex_token>",
  "ibkr_query_id": "<optional_ibkr_query_id>",
  "finpension_token": "<optional_finpension_bearer_token>",
  "uphold_token": "<optional_uphold_bearer_token>",
  "marketwatch_urls": [
    "GME",
    "SOFI",
    "https://www.marketwatch.com/investing/stock/xmrusd"
  ],
  "coinmarketcap_urls": [
    "https://coinmarketcap.com/currencies/bitcoin/",
    "https://coinmarketcap.com/currencies/ethereum/",
    "https://coinmarketcap.com/currencies/monero/"
  ]
}
```

---

## Running Tests

```bash
cargo test
```

