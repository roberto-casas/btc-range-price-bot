use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single Polymarket "BTC above $X" market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub question: String,
    pub threshold: f64,
    pub ratio: f64,
    pub end_date: DateTime<Utc>,
    pub days_until: i64,
    pub yes_price: f64,
    pub yes_token_id: String,
    pub no_token_id: Option<String>,
    pub leg: Leg,
    pub slug: String,
}

/// Whether this market is the low or high leg of a delta-neutral pair
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Leg {
    Low,
    High,
}

/// A matched delta-neutral pair (low + high leg)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    pub low_market: Market,
    pub high_market: Market,
    pub yes_price_low: f64,
    pub yes_price_high: f64,
    pub profit_in_rng: f64,
    pub profit_pct: f64,
    pub rr_reward: f64,
}

/// Calculated structure metrics for a pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Structure {
    pub yes_price_low: f64,
    pub yes_price_high: f64,
    pub no_price: f64,
    pub cost_per_unit: f64,
    pub profit_in_rng: f64,
    pub rr_reward: f64,
    pub profit_pct: f64,
    pub balance: f64,
    pub per_leg: f64,
    pub units: f64,
    pub cost_low: f64,
    pub cost_high: f64,
    pub total_cost: f64,
    pub expected_profit: f64,
    pub max_drawdown: f64,
}

/// Output record for the dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPair {
    pub low_threshold: f64,
    pub high_threshold: f64,
    pub low_question: String,
    pub high_question: String,
    pub low_url: String,
    pub high_url: String,
    pub expiry: String,
    pub expiry_date: String,
    pub days_until: i64,
    pub yes_price_low: f64,
    pub yes_price_high: f64,
    pub no_price: f64,
    pub cost_per_unit: f64,
    pub cost_low: f64,
    pub cost_high: f64,
    pub profit_in_rng: f64,
    pub profit_pct: f64,
    pub rr_reward: f64,
    pub low_pct: f64,
    pub high_pct: f64,
}

/// Full scan result (returned via API and used by dashboard)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub generated_at: String,
    pub btc_price: f64,
    pub pairs: Vec<OutputPair>,
    pub dry_run: bool,
}

/// A historical candle / OHLC data point for backtesting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub timestamp: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Result of simulating a single pair trade in backtesting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestTrade {
    pub entry_date: DateTime<Utc>,
    pub expiry_date: DateTime<Utc>,
    pub low_threshold: f64,
    pub high_threshold: f64,
    pub entry_cost: f64,
    pub profit_in_rng: f64,
    pub profit_pct: f64,
    pub won: bool,
    pub btc_at_expiry: f64,
    pub pnl: f64,
    /// True if trade was closed by stop-loss before expiry
    #[serde(default)]
    pub stopped_out: bool,
    /// True if trade was closed by take-profit before expiry
    #[serde(default)]
    pub took_profit_early: bool,
}

/// Aggregated backtest summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestSummary {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_profit_pct: f64,
    pub trades: Vec<BacktestTrade>,
    /// Number of trades closed early by stop-loss
    #[serde(default)]
    pub stopped_out: usize,
    /// Number of trades closed early by take-profit
    #[serde(default)]
    pub took_profit: usize,
}

/// Configuration for backtesting (all parameters in one place).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub low_ratio: f64,
    pub high_ratio: f64,
    pub duration_days: i64,
    pub yes_price_low: f64,
    pub yes_price_high: f64,
    /// Stop-loss: close trade if BTC moves this far outside the range (% of spot).
    /// E.g. 0.15 = close if BTC drops 15% below low_threshold or rises 15% above high_threshold.
    /// None = no stop-loss (hold to expiry).
    pub stop_loss_pct: Option<f64>,
    /// Take-profit: close trade early if unrealized profit exceeds this fraction
    /// of max possible profit. E.g. 0.8 = take profit at 80% of max gain.
    /// None = hold to expiry.
    pub take_profit_pct: Option<f64>,
    /// Data interval for sampling: "daily", "weekly", or "monthly".
    /// Controls how frequently new trades are opened.
    pub entry_interval: String,
}

impl Default for BacktestConfig {
    /// Optimized defaults based on historical backtest (Jan 2023–Apr 2025, 821 candles):
    ///
    /// | Config                          | WR%   | Sharpe | EV/trade | Edge    |
    /// |---------------------------------|-------|--------|----------|---------|
    /// | 92-108%, SL5%, TP80%, weekly    | 98.3% | 26.59  | 0.85     | +53.3pp |
    /// | 92-108%, SL5%, TP80%, daily     | 98.9% | 88.31  | 1.08     | +53.9pp |
    /// | 90-110%, 7d, daily (old default)| 96.9% | 57.50  | 1.04     | +51.9pp |
    ///
    /// Weekly entry reduces exposure while keeping >98% win rate.
    fn default() -> Self {
        Self {
            low_ratio: 0.92,
            high_ratio: 1.08,
            duration_days: 7,
            yes_price_low: 0.55,
            yes_price_high: 0.65,
            stop_loss_pct: Some(0.05),
            take_profit_pct: Some(0.80),
            entry_interval: "weekly".to_string(),
        }
    }
}

/// A simulated order (dry-run mode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatedOrder {
    pub pair_label: String,
    pub leg: String,
    pub side: String,
    pub token_id: String,
    pub price: f64,
    pub units: f64,
    pub cost: f64,
    pub dry_run: bool,
}
