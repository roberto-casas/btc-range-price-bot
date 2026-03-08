# PolyDelta — BTC Range Price Bot (Rust)

A **delta-neutral BTC range strategy finder** for [Polymarket](https://polymarket.com), rewritten in Rust from the original Python [polydelta](https://github.com/st1ne/polydelta) project.

![Scanner Dashboard](https://github.com/user-attachments/assets/b87fcb19-5a23-4fa4-b05e-49d30384df4d)
![Backtesting Tab](https://github.com/user-attachments/assets/4550cb46-bb88-43b7-8624-c990706039a3)

## Strategy Overview

The strategy pairs two Polymarket prediction markets to profit when BTC stays inside a price range:

- **Leg A (LOW):** BUY YES on "BTC above $64,000" → wins if BTC > $64 k at expiry  
- **Leg B (HIGH):** BUY NO on "BTC above $70,000" → wins if BTC < $70 k at expiry  

**Win condition:** BTC price remains within [$64 k, $70 k] at expiration.

## Features

| Feature | Description |
|---|---|
| 🔍 **Market scanner** | Fetches live Polymarket markets via Gamma API + CLOB API |
| 📡 **WebSocket feed** | Live price updates from Polymarket CLOB WebSocket |
| 🖥️ **Web dashboard** | Built-in HTTP server with a dark-themed interactive UI |
| 🟡 **Dry-run mode** | Simulate order placement without touching any real funds |
| 📈 **Backtesting** | Replay the strategy against historical BTC price data |
| ♻️ **Auto-refresh** | Configurable scan interval with automatic dashboard updates |

## Security Notes

The original Python `polydelta` project was inspected for backdoors:

- **No malicious code** was found in the scanning logic or API calls.
- **Referral links** (`?via=SolSt1ne`) were embedded in every generated market URL, silently routing affiliate commissions to the author. These have been **removed** in this implementation — all market URLs are clean.
- All APIs used (CoinGecko, Polymarket Gamma, CLOB) are public and unauthenticated.

## Quick Start

### Prerequisites

- Rust 1.75+ (`rustup` — https://rustup.rs)

### Build

```bash
git clone https://github.com/roberto-casas/btc-range-price-bot
cd btc-range-price-bot
cargo build --release
```

### Run the scanner + dashboard

```bash
# Start scanner with web dashboard on port 8080 (re-scans every 5 minutes)
./target/release/polydelta scan

# Custom options
./target/release/polydelta scan \
  --port 8080 \
  --timeframe week \
  --interval 300 \
  --balance 100 \
  --dry-run \
  --live
```

Then open **http://localhost:8080** in your browser.

### Dry-run mode

```bash
./target/release/polydelta scan --dry-run
```

All simulated orders are printed to stdout and a banner is shown in the dashboard. No real API calls for order placement are made.

### Live WebSocket feed

```bash
./target/release/polydelta scan --live
```

Connects to the Polymarket CLOB WebSocket and streams real-time price updates.

### Run a backtest (CLI)

```bash
./target/release/polydelta backtest \
  --low-pct 90 \
  --high-pct 110 \
  --duration-days 7 \
  --yes-price-low 0.60 \
  --yes-price-high 0.70 \
  --history-days 90
```

Fetches 90 days of daily BTC candles from CoinGecko and simulates the strategy for every possible entry window.

### Run a backtest (Dashboard)

1. Open the dashboard in your browser.
2. Click the **📈 Backtesting** tab.
3. Adjust the parameters and click **▶ Run Backtest**.

Results appear immediately in the browser with a win-rate summary and a trade table.

## API Reference

| Endpoint | Description |
|---|---|
| `GET /` | Dashboard UI |
| `GET /api/health` | Health check — returns `{"status":"ok"}` |
| `GET /api/data` | Latest scan results as JSON |
| `GET /api/backtest?...` | Run a backtest — see parameters below |

**Backtest query parameters:**

| Parameter | Default | Description |
|---|---|---|
| `low_ratio` | `0.90` | Lower price bound as fraction of spot |
| `high_ratio` | `1.10` | Upper price bound as fraction of spot |
| `duration` | `7` | Trade holding period in days |
| `yes_price_low` | `0.60` | Assumed YES-leg entry price |
| `yes_price_high` | `0.70` | Assumed HIGH-leg YES price |
| `history_days` | `90` | Days of historical data to fetch |

## Project Structure

```
src/
├── main.rs           # CLI entry point (clap subcommands)
├── types.rs          # Shared data types and models
├── scanner.rs        # Polymarket + CoinGecko API client, pair logic
├── polymarket_ws.rs  # Polymarket CLOB WebSocket listener
├── dashboard.rs      # Axum web server + embedded dashboard HTML
├── backtesting.rs    # Historical backtest engine
└── dry_run.rs        # Dry-run order simulation
```

## Configuration

| CLI flag | Default | Description |
|---|---|---|
| `--port` | `8080` | Dashboard HTTP port |
| `--timeframe` | `week` | `week` or `month` |
| `--interval` | `300` | Seconds between scans (0 = scan once) |
| `--balance` | `100` | USDC capital for cost breakdown |
| `--dry-run` | off | Simulate trades only |
| `--live` | off | Enable WebSocket price feed |

## Configuración con fichero `bot-config.json`

Ahora el bot carga parámetros por defecto desde `bot-config.json` (raíz del proyecto).
Puedes ajustar ahí los valores de `scan` y `backtest` sin tocar el código.

```bash
# usar config por defecto
./target/release/polydelta scan

# usar otro fichero
./target/release/polydelta --config ./mi-config.json backtest
```

Las flags CLI siguen teniendo prioridad sobre el fichero.

Ejemplo de fichero:

```json
{
  "scan": {
    "timeframe": "week",
    "port": 8080,
    "balance": 100.0,
    "interval": 300,
    "dry_run": false,
    "live": false
  },
  "backtest": {
    "low_pct": 92.0,
    "high_pct": 108.0,
    "duration_days": 7,
    "yes_price_low": 0.55,
    "yes_price_high": 0.65,
    "history_days": 90,
    "stop_loss": 5.0,
    "take_profit": 80.0,
    "interval": "weekly",
    "offline": false
  }
}
```

## License

MIT
