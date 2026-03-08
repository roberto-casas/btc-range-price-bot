use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub scan: ScanConfig,
    pub backtest: BacktestConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scan: ScanConfig::default(),
            backtest: BacktestConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ScanConfig {
    pub timeframe: String,
    pub port: u16,
    pub balance: f64,
    pub interval: u64,
    pub dry_run: bool,
    pub live: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            timeframe: "week".to_string(),
            port: 8080,
            balance: 100.0,
            interval: 300,
            dry_run: false,
            live: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BacktestConfig {
    pub low_pct: f64,
    pub high_pct: f64,
    pub duration_days: i64,
    pub yes_price_low: f64,
    pub yes_price_high: f64,
    pub history_days: u32,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub interval: String,
    pub offline: bool,
    pub spread: f64,
    pub slippage: f64,
    pub fee: f64,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            low_pct: 88.0,
            high_pct: 112.0,
            duration_days: 7,
            yes_price_low: 0.85,
            yes_price_high: 0.15,
            history_days: 90,
            stop_loss: 5.0,
            take_profit: 0.0,
            interval: "weekly".to_string(),
            offline: false,
            spread: 2.0,
            slippage: 0.5,
            fee: 0.0,
        }
    }
}

pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("Could not read config file: {}", path.display()))?;

    serde_json::from_str::<AppConfig>(&raw)
        .with_context(|| format!("Invalid JSON config format in {}", path.display()))
}
