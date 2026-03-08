use crate::types::{BacktestConfig, BacktestSummary, BacktestTrade, Candle};
use chrono::{DateTime, Datelike, Duration, Utc};
use tracing::info;

/// Simulate the delta-neutral BTC range strategy against historical daily candles.
///
/// For each entry point in the history we:
/// 1. Simulate an entry at that candle's close price.
/// 2. Define thresholds based on low/high ratios.
/// 3. During holding period, check for stop-loss/take-profit triggers using intraday H/L.
/// 4. At expiry, check if BTC is in range.
/// 5. Record the trade outcome.
pub fn run_backtest(
    candles: &[Candle],
    low_ratio: f64,
    high_ratio: f64,
    duration_days: i64,
    yes_price_low: f64,
    yes_price_high: f64,
) -> BacktestSummary {
    let config = BacktestConfig {
        low_ratio,
        high_ratio,
        duration_days,
        yes_price_low,
        yes_price_high,
        stop_loss_pct: None,
        take_profit_pct: None,
        entry_interval: "daily".to_string(),
    };
    run_backtest_advanced(candles, &config)
}

/// Run backtest with full configuration including stop-loss, take-profit,
/// and configurable entry intervals.
pub fn run_backtest_advanced(candles: &[Candle], config: &BacktestConfig) -> BacktestSummary {
    let empty = BacktestSummary {
        total_trades: 0,
        winning_trades: 0,
        losing_trades: 0,
        win_rate: 0.0,
        total_pnl: 0.0,
        avg_profit_pct: 0.0,
        trades: vec![],
        stopped_out: 0,
        took_profit: 0,
    };

    if candles.is_empty() {
        return empty;
    }

    let no_price = 1.0 - config.yes_price_high;
    let cost_per_unit = config.yes_price_low + no_price;
    let profit_in_rng = 2.0 - cost_per_unit;
    let profit_pct = (profit_in_rng / cost_per_unit) * 100.0;

    let mut trades: Vec<BacktestTrade> = Vec::new();

    // Determine entry indices based on interval
    let entry_indices = select_entry_indices(candles, &config.entry_interval, config.duration_days);

    for i in entry_indices {
        let entry = &candles[i];
        let spot = entry.close;

        let low_threshold = spot * config.low_ratio;
        let high_threshold = spot * config.high_ratio;
        let expiry_date: DateTime<Utc> = entry.timestamp + Duration::days(config.duration_days);

        // Check during holding period for stop-loss / take-profit
        let (final_price, stopped_out, took_profit_early, exit_date) = simulate_holding_period(
            candles,
            i,
            config.duration_days,
            low_threshold,
            high_threshold,
            cost_per_unit,
            profit_in_rng,
            config.stop_loss_pct,
            config.take_profit_pct,
        );

        let btc_at_expiry = final_price;
        let actual_expiry = exit_date.unwrap_or(expiry_date);

        // Determine outcome
        let (won, pnl) = if stopped_out {
            // Stop-loss triggered: lose entry cost
            (false, -cost_per_unit)
        } else if took_profit_early {
            // Take-profit triggered: partial profit
            let tp_fraction = config.take_profit_pct.unwrap_or(1.0);
            (true, profit_in_rng * tp_fraction)
        } else {
            // Hold to expiry: check if in range
            let in_range = btc_at_expiry >= low_threshold && btc_at_expiry <= high_threshold;
            if in_range {
                (true, profit_in_rng)
            } else {
                (false, -cost_per_unit)
            }
        };

        trades.push(BacktestTrade {
            entry_date: entry.timestamp,
            expiry_date: actual_expiry,
            low_threshold,
            high_threshold,
            entry_cost: cost_per_unit,
            profit_in_rng,
            profit_pct,
            won,
            btc_at_expiry,
            pnl,
            stopped_out,
            took_profit_early,
        });
    }

    let total_trades = trades.len();
    let winning_trades = trades.iter().filter(|t| t.won).count();
    let losing_trades = total_trades - winning_trades;
    let total_pnl: f64 = trades.iter().map(|t| t.pnl).sum();
    let stopped_out = trades.iter().filter(|t| t.stopped_out).count();
    let took_profit = trades.iter().filter(|t| t.took_profit_early).count();
    let avg_profit_pct = if total_trades > 0 {
        trades
            .iter()
            .map(|t| (t.pnl / t.entry_cost) * 100.0)
            .sum::<f64>()
            / total_trades as f64
    } else {
        0.0
    };
    let win_rate = if total_trades > 0 {
        winning_trades as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };

    info!(
        "Backtest complete: {} trades, {:.1}% win rate, total PnL: {:.4} (SL:{} TP:{})",
        total_trades, win_rate, total_pnl, stopped_out, took_profit
    );

    BacktestSummary {
        total_trades,
        winning_trades,
        losing_trades,
        win_rate,
        total_pnl,
        avg_profit_pct,
        trades,
        stopped_out,
        took_profit,
    }
}

/// Select which candle indices to use as entry points based on the interval.
fn select_entry_indices(candles: &[Candle], interval: &str, duration_days: i64) -> Vec<usize> {
    let max_start = candles.len().saturating_sub(duration_days as usize);
    if max_start == 0 {
        return vec![];
    }

    match interval {
        "weekly" => {
            // Enter once per week (every 7 candles, or on Monday)
            candles
                .iter()
                .enumerate()
                .take(max_start)
                .filter(|(_, c)| c.timestamp.weekday() == chrono::Weekday::Mon)
                .map(|(i, _)| i)
                .collect()
        }
        "monthly" => {
            // Enter once per month (first trading day of each month)
            let mut result = Vec::new();
            let mut last_month = (0i32, 0u32);
            for (i, c) in candles.iter().enumerate().take(max_start) {
                let d = c.timestamp.date_naive();
                let ym = (d.year(), d.month());
                if ym != last_month {
                    result.push(i);
                    last_month = ym;
                }
            }
            result
        }
        _ => {
            // "daily" — every candle
            (0..max_start).collect()
        }
    }
}

/// Simulate the holding period, checking intraday highs/lows for SL/TP triggers.
///
/// Returns: (final_price, stopped_out, took_profit, exit_date)
fn simulate_holding_period(
    candles: &[Candle],
    entry_idx: usize,
    duration_days: i64,
    low_threshold: f64,
    high_threshold: f64,
    _cost_per_unit: f64,
    _profit_in_rng: f64,
    stop_loss_pct: Option<f64>,
    take_profit_pct: Option<f64>,
) -> (f64, bool, bool, Option<DateTime<Utc>>) {
    let entry_ts = candles[entry_idx].timestamp;
    let expiry_ts = entry_ts + Duration::days(duration_days);

    // Stop-loss thresholds: BTC moves far enough outside range that the trade is clearly lost
    let sl_low = stop_loss_pct.map(|pct| low_threshold * (1.0 - pct));
    let sl_high = stop_loss_pct.map(|pct| high_threshold * (1.0 + pct));

    // Scan candles during holding period
    for c in candles.iter().skip(entry_idx + 1) {
        if c.timestamp > expiry_ts {
            break;
        }

        // Check stop-loss: did intraday price breach SL levels?
        if let (Some(sl_l), Some(sl_h)) = (sl_low, sl_high) {
            if c.low < sl_l || c.high > sl_h {
                return (c.close, true, false, Some(c.timestamp));
            }
        }

        // Check take-profit: is BTC perfectly centered in range (high confidence of win)?
        // TP triggers if the current price is within the inner portion of the range
        if let Some(tp_frac) = take_profit_pct {
            let range_width = high_threshold - low_threshold;
            let inner_margin = range_width * (1.0 - tp_frac) / 2.0;
            let tp_low = low_threshold + inner_margin;
            let tp_high = high_threshold - inner_margin;
            if c.close >= tp_low && c.close <= tp_high {
                // Only trigger TP if we're past 50% of the holding period
                let elapsed = (c.timestamp - entry_ts).num_days() as f64;
                let total = duration_days as f64;
                if elapsed / total >= 0.5 {
                    return (c.close, false, true, Some(c.timestamp));
                }
            }
        }
    }

    // No early exit — find expiry candle
    let expiry_candle = candles
        .iter()
        .skip(entry_idx)
        .find(|c| c.timestamp >= expiry_ts);

    let final_price = match expiry_candle {
        Some(c) => c.close,
        None => candles.last().unwrap().close,
    };

    (final_price, false, false, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BacktestConfig;
    use chrono::TimeZone;

    fn make_candles(prices: &[f64]) -> Vec<Candle> {
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle {
                timestamp: base + Duration::days(i as i64),
                open: p,
                high: p * 1.01,
                low: p * 0.99,
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
        let prices: Vec<f64> = vec![100.0; 30];
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);
        assert!(summary.total_trades > 0);
        assert_eq!(summary.winning_trades, summary.total_trades);
        assert!((summary.win_rate - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_backtest_all_losses() {
        let mut prices: Vec<f64> = vec![100.0; 10];
        prices.extend(vec![50.0; 20]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);
        assert!(summary.total_trades > 0);
        assert!(summary.losing_trades > 0);
    }

    #[test]
    fn test_calculate_structure_metrics() {
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
        let expected_pnl = summary.winning_trades as f64 * 1.1;
        assert!((summary.total_pnl - expected_pnl).abs() < 1e-6);
    }

    #[test]
    fn test_stop_loss() {
        // BTC starts at 100, crashes to 50 on day 3 (well below 90% range)
        let mut prices: Vec<f64> = vec![100.0; 3];
        prices.push(50.0); // day 3: crash
        prices.extend(vec![50.0; 20]);
        let candles = make_candles(&prices);

        let config = BacktestConfig {
            low_ratio: 0.90,
            high_ratio: 1.10,
            duration_days: 7,
            yes_price_low: 0.60,
            yes_price_high: 0.70,
            stop_loss_pct: Some(0.05), // 5% beyond range = SL
            take_profit_pct: None,
            entry_interval: "daily".to_string(),
        };

        let summary = run_backtest_advanced(&candles, &config);
        // Trades entered on day 0 should be stopped out when price crashes
        assert!(summary.stopped_out > 0, "Expected some stopped-out trades");
        println!("SL test: {} trades, {} stopped out", summary.total_trades, summary.stopped_out);
    }

    #[test]
    fn test_weekly_interval() {
        let prices: Vec<f64> = vec![100.0; 60];
        let candles = make_candles(&prices);

        let daily = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);
        let config_weekly = BacktestConfig {
            low_ratio: 0.90,
            high_ratio: 1.10,
            duration_days: 7,
            yes_price_low: 0.60,
            yes_price_high: 0.70,
            stop_loss_pct: None,
            take_profit_pct: None,
            entry_interval: "weekly".to_string(),
        };
        let weekly = run_backtest_advanced(&candles, &config_weekly);

        assert!(weekly.total_trades < daily.total_trades,
            "Weekly ({}) should have fewer trades than daily ({})",
            weekly.total_trades, daily.total_trades);
        println!("Daily: {} trades, Weekly: {} trades", daily.total_trades, weekly.total_trades);
    }

    #[test]
    fn test_monthly_interval() {
        // Generate 120 candles spanning ~4 months
        let candles = crate::historical_data::generate_embedded_candles();

        let config = BacktestConfig {
            low_ratio: 0.90,
            high_ratio: 1.10,
            duration_days: 7,
            yes_price_low: 0.60,
            yes_price_high: 0.70,
            stop_loss_pct: None,
            take_profit_pct: None,
            entry_interval: "monthly".to_string(),
        };
        let monthly = run_backtest_advanced(&candles, &config);

        // Should have roughly 1 trade per month
        assert!(monthly.total_trades > 10, "Expected >10 monthly trades over ~2 years");
        assert!(monthly.total_trades < 50, "Expected <50 monthly trades over ~2 years, got {}", monthly.total_trades);
        println!("Monthly: {} trades", monthly.total_trades);
    }

    #[test]
    fn test_backtest_realistic_btc_data() {
        let mut prices = Vec::with_capacity(90);
        let base = 85000.0_f64;
        for i in 0..30 {
            let noise = ((i * 7 + 3) % 11) as f64 * 100.0 - 500.0;
            prices.push(base + (i as f64) * 333.0 + noise);
        }
        for i in 0..30 {
            let noise = ((i * 13 + 5) % 17) as f64 * 150.0 - 1200.0;
            prices.push(94000.0 + noise);
        }
        for i in 0..30 {
            let dip = if i < 15 {
                -800.0 * i as f64
            } else {
                -12000.0 + 600.0 * (i - 15) as f64
            };
            let noise = ((i * 11 + 7) % 13) as f64 * 80.0 - 500.0;
            prices.push(94000.0 + dip + noise);
        }

        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.60, 0.70);

        assert_eq!(summary.total_trades, 83);
        assert_eq!(summary.total_trades, summary.winning_trades + summary.losing_trades);
        assert!(summary.win_rate >= 0.0 && summary.win_rate <= 100.0);

        for trade in &summary.trades {
            assert!(trade.low_threshold < trade.high_threshold);
            assert!(trade.entry_cost > 0.0);
        }
    }

    #[test]
    fn test_avg_profit_pct_reflects_actual_outcomes() {
        let mut prices: Vec<f64> = vec![100.0; 15];
        prices.extend(vec![80.0; 15]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);

        assert!(summary.winning_trades > 0);
        assert!(summary.losing_trades > 0);

        let cost = 0.6 + (1.0 - 0.7);
        let win_pct = ((2.0 - cost) / cost) * 100.0;
        let loss_pct = (-cost / cost) * 100.0;

        let expected_avg = (summary.winning_trades as f64 * win_pct
            + summary.losing_trades as f64 * loss_pct)
            / summary.total_trades as f64;
        assert!(
            (summary.avg_profit_pct - expected_avg).abs() < 0.01,
            "avg_profit_pct should reflect actual outcomes: got {}, expected {}",
            summary.avg_profit_pct,
            expected_avg
        );
    }
}
