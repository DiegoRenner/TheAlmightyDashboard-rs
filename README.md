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
  * **Monero**: Reads pending mining pool rewards from MoneroOcean + local wallet RPC balance, valued in USD with live XMR quotes.
  * **Uphold**: Fetches positive card balances and computes live USD valuation.
  * **Coinbase**: Reads connected account balances.
  * **Total Valuation**: Sums all held assets into a single highlighted USD total.
* **Modern Terminal UI**:
  * Built with Ratatui and Crossterm (pure terminal raw mode, non-blocking, responsive).
  * Auto-adapts side-by-side or stacked layout based on terminal size.
  * Includes the classic "make biggr pls" easter egg when the window is too small.
  * Safe panic hook to guarantee terminal restoration upon exit or unexpected termination.
* **Keyboard Navigation**:
  * `q` / `Q`: Exit cleanly (restores cursor and terminal state)
  * `j` / `Down Arrow`: Scroll down
  * `k` / `Up Arrow`: Scroll up

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

The Rust dashboard uses the exact same JSON format as the Python version:

```json
{
  "marketwatch_urls": [
    "https://www.marketwatch.com/investing/stock/gme",
    "SOFI",
    "AMC",
    "BB",
    "https://www.marketwatch.com/investing/stock/xmrusd"
  ],
  "coinmarketcap_urls": [
    "https://coinmarketcap.com/currencies/bitcoin/",
    "https://coinmarketcap.com/currencies/ethereum/",
    "https://coinmarketcap.com/currencies/monero/"
  ],
  "monero_wallet_address": "<optional_address>",
  "uphold_token": "<optional_uphold_bearer_token>",
  "coinbase_api_key": "<optional_coinbase_key>",
  "coinbase_api_secret": "<optional_coinbase_secret>",
  "update_frequency_secs": 5.0
}
```

---

## Running Tests

```bash
cargo test
```
