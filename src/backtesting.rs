use crate::types::{BacktestSummary, BacktestTrade, Candle};
use chrono::{DateTime, Duration, Utc};
use tracing::info;

/// Simulate the delta-neutral BTC range strategy against historical daily candles.
///
/// For each day in the history we:
/// 1. Simulate an entry at that day's BTC close price.
/// 2. Generate synthetic market prices based on configurable low/high ratios.
/// 3. Fast-forward `duration_days` and check if BTC stayed in range.
/// 4. Record the trade outcome.
///
/// # Parameters
/// - `candles`        – Daily BTC price history (oldest first).
/// - `low_ratio`      – Lower bound as fraction of spot (e.g. 0.90 = 90%).
/// - `high_ratio`     – Upper bound as fraction of spot (e.g. 1.10 = 110%).
/// - `duration_days`  – Holding period in days.
/// - `yes_price_low`  – Assumed YES-leg entry price (0..1).
/// - `yes_price_high` – Assumed HIGH-leg YES price (0..1).
pub fn run_backtest(
    candles: &[Candle],
    low_ratio: f64,
    high_ratio: f64,
    duration_days: i64,
    yes_price_low: f64,
    yes_price_high: f64,
) -> BacktestSummary {
    if candles.is_empty() {
        return BacktestSummary {
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
            win_rate: 0.0,
            total_pnl: 0.0,
            avg_profit_pct: 0.0,
            trades: vec![],
        };
    }

    let no_price = 1.0 - yes_price_high;
    let cost_per_unit = yes_price_low + no_price;
    let profit_in_rng = 2.0 - cost_per_unit;
    let profit_pct = (profit_in_rng / cost_per_unit) * 100.0;

    let mut trades: Vec<BacktestTrade> = Vec::new();

    // For each possible entry candle (except the last `duration_days` candles)
    for i in 0..(candles.len().saturating_sub(duration_days as usize)) {
        let entry = &candles[i];
        let spot = entry.close;

        let low_threshold = spot * low_ratio;
        let high_threshold = spot * high_ratio;

        let expiry_date: DateTime<Utc> = entry.timestamp + Duration::days(duration_days);

        // Find the candle closest to expiry
        let expiry_candle = candles
            .iter()
            .skip(i)
            .find(|c| c.timestamp >= expiry_date);

        let btc_at_expiry = match expiry_candle {
            Some(c) => c.close,
            None => candles.last().unwrap().close,
        };

        let won = btc_at_expiry >= low_threshold && btc_at_expiry <= high_threshold;
        let pnl = if won { profit_in_rng } else { -cost_per_unit };

        trades.push(BacktestTrade {
            entry_date: entry.timestamp,
            expiry_date,
            low_threshold,
            high_threshold,
            entry_cost: cost_per_unit,
            profit_in_rng,
            profit_pct,
            won,
            btc_at_expiry,
            pnl,
        });
    }

    let total_trades = trades.len();
    let winning_trades = trades.iter().filter(|t| t.won).count();
    let losing_trades = total_trades - winning_trades;
    let total_pnl: f64 = trades.iter().map(|t| t.pnl).sum();
    let avg_profit_pct = if total_trades > 0 {
        trades.iter().map(|t| t.profit_pct).sum::<f64>() / total_trades as f64
    } else {
        0.0
    };
    let win_rate = if total_trades > 0 {
        winning_trades as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };

    info!(
        "Backtest complete: {} trades, {:.1}% win rate, total PnL: {:.4}",
        total_trades, win_rate, total_pnl
    );

    BacktestSummary {
        total_trades,
        winning_trades,
        losing_trades,
        win_rate,
        total_pnl,
        avg_profit_pct,
        trades,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_candles(prices: &[f64]) -> Vec<Candle> {
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle {
                timestamp: base + Duration::days(i as i64),
                open: p,
                high: p,
                low: p,
                close: p,
            })
            .collect()
    }

    #[test]
    fn test_backtest_empty() {
        let summary = run_backtest(&[], 0.90, 1.10, 7, 0.6, 0.7);
        assert_eq!(summary.total_trades, 0);
    }

    #[test]
    fn test_backtest_all_wins() {
        // BTC stays flat at 100, range is 90-110, all trades win
        let prices: Vec<f64> = vec![100.0; 30];
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);
        assert!(summary.total_trades > 0);
        assert_eq!(summary.winning_trades, summary.total_trades);
        assert!((summary.win_rate - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_backtest_all_losses() {
        // BTC starts at 100 then crashes to 50 — below 90% range
        let mut prices: Vec<f64> = vec![100.0; 10];
        prices.extend(vec![50.0; 20]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);
        assert!(summary.total_trades > 0);
        assert!(summary.losing_trades > 0);
    }

    #[test]
    fn test_calculate_structure_metrics() {
        // no_price = 1 - 0.7 = 0.3; cost = 0.6 + 0.3 = 0.9; profit = 2 - 0.9 = 1.1
        let no_price = 1.0 - 0.7_f64;
        let cost = 0.6_f64 + no_price;
        let profit = 2.0 - cost;
        assert!((profit - 1.1).abs() < 1e-9);
    }

    #[test]
    fn test_backtest_profit_calculation() {
        let prices: Vec<f64> = vec![100.0; 30];
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);
        // Each win: pnl = profit_in_rng = 2 - (0.6 + 0.3) = 1.1
        let expected_pnl = summary.winning_trades as f64 * 1.1;
        assert!((summary.total_pnl - expected_pnl).abs() < 1e-6);
    }
}
