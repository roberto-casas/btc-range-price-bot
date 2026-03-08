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
