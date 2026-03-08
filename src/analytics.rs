//! Advanced financial analytics for the delta-neutral range strategy.
//!
//! Provides: Kelly criterion, Sharpe/Sortino ratios, drawdown analysis,
//! volatility metrics, and Monte Carlo simulation.

use crate::types::{BacktestSummary, Candle};
use serde::{Deserialize, Serialize};

// ── Kelly Criterion ─────────────────────────────────────────────────────────

/// Kelly criterion result for optimal position sizing.
///
/// Formula: f* = (b*p - q) / b
/// where: b = win/loss ratio, p = win probability, q = 1-p
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KellyResult {
    /// Full Kelly fraction (fraction of bankroll to bet)
    pub full_kelly: f64,
    /// Half Kelly (more conservative, commonly used)
    pub half_kelly: f64,
    /// Quarter Kelly (very conservative)
    pub quarter_kelly: f64,
    /// Estimated edge (expected return per dollar risked)
    pub edge: f64,
    /// Win probability used
    pub win_prob: f64,
    /// Win/loss ratio (b)
    pub win_loss_ratio: f64,
}

/// Calculate Kelly criterion from backtest results.
///
/// For binary outcomes: win pays `profit_in_rng`, loss costs `entry_cost`.
pub fn kelly_criterion(summary: &BacktestSummary) -> KellyResult {
    if summary.total_trades == 0 || summary.winning_trades == 0 {
        return KellyResult {
            full_kelly: 0.0,
            half_kelly: 0.0,
            quarter_kelly: 0.0,
            edge: 0.0,
            win_prob: 0.0,
            win_loss_ratio: 0.0,
        };
    }

    let p = summary.winning_trades as f64 / summary.total_trades as f64;
    let q = 1.0 - p;

    // b = average_win / average_loss
    let avg_win: f64 = summary
        .trades
        .iter()
        .filter(|t| t.won)
        .map(|t| t.pnl)
        .sum::<f64>()
        / summary.winning_trades as f64;

    let avg_loss: f64 = if summary.losing_trades > 0 {
        summary
            .trades
            .iter()
            .filter(|t| !t.won)
            .map(|t| t.pnl.abs())
            .sum::<f64>()
            / summary.losing_trades as f64
    } else {
        // No losses observed — use entry cost as theoretical max loss
        summary
            .trades
            .first()
            .map(|t| t.entry_cost)
            .unwrap_or(1.0)
    };

    let b = avg_win / avg_loss;
    let full_kelly = (b * p - q) / b;
    let edge = b * p - q; // Expected value per unit risked

    KellyResult {
        full_kelly: full_kelly.max(0.0), // Never bet negative
        half_kelly: (full_kelly / 2.0).max(0.0),
        quarter_kelly: (full_kelly / 4.0).max(0.0),
        edge,
        win_prob: p,
        win_loss_ratio: b,
    }
}

// ── Risk-Adjusted Metrics ───────────────────────────────────────────────────

/// Comprehensive risk metrics for a backtest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    /// Annualized Sharpe ratio (assuming 365 trading days for crypto)
    pub sharpe_ratio: f64,
    /// Sortino ratio (penalizes only downside volatility)
    pub sortino_ratio: f64,
    /// Maximum drawdown as fraction of peak equity
    pub max_drawdown_pct: f64,
    /// Maximum drawdown in absolute terms
    pub max_drawdown_abs: f64,
    /// Calmar ratio (annualized return / max drawdown)
    pub calmar_ratio: f64,
    /// Profit factor (gross profit / gross loss)
    pub profit_factor: f64,
    /// Standard deviation of returns
    pub return_std: f64,
    /// Downside deviation (for Sortino)
    pub downside_std: f64,
    /// Equity curve (cumulative PnL at each trade)
    pub equity_curve: Vec<f64>,
    /// Drawdown curve (drawdown at each point)
    pub drawdown_curve: Vec<f64>,
}

pub fn calculate_risk_metrics(summary: &BacktestSummary) -> RiskMetrics {
    if summary.total_trades < 2 {
        return RiskMetrics {
            sharpe_ratio: 0.0,
            sortino_ratio: 0.0,
            max_drawdown_pct: 0.0,
            max_drawdown_abs: 0.0,
            calmar_ratio: 0.0,
            profit_factor: 0.0,
            return_std: 0.0,
            downside_std: 0.0,
            equity_curve: vec![],
            drawdown_curve: vec![],
        };
    }

    // Returns per trade (as fraction of entry cost)
    let returns: Vec<f64> = summary
        .trades
        .iter()
        .map(|t| t.pnl / t.entry_cost)
        .collect();

    let n = returns.len() as f64;
    let mean_return = returns.iter().sum::<f64>() / n;

    // Standard deviation
    let variance = returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / (n - 1.0);
    let return_std = variance.sqrt();

    // Downside deviation (only negative returns)
    let downside_variance = returns
        .iter()
        .filter(|&&r| r < 0.0)
        .map(|r| r.powi(2))
        .sum::<f64>()
        / n; // Use full n for consistency
    let downside_std = downside_variance.sqrt();

    // Annualization factor: assume average trade duration from data
    let avg_duration_days = if summary.trades.len() >= 2 {
        let first = summary.trades.first().unwrap().entry_date;
        let last = summary.trades.last().unwrap().entry_date;
        let span = (last - first).num_days().max(1) as f64;
        span / summary.total_trades as f64
    } else {
        7.0
    };
    let trades_per_year = 365.0 / avg_duration_days;
    let annualization = trades_per_year.sqrt();

    // Sharpe ratio (risk-free rate ≈ 0 for simplicity in crypto context)
    let sharpe_ratio = if return_std > 0.0 {
        (mean_return / return_std) * annualization
    } else {
        0.0
    };

    // Sortino ratio
    let sortino_ratio = if downside_std > 0.0 {
        (mean_return / downside_std) * annualization
    } else if mean_return > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    // Equity curve and drawdown
    let mut equity_curve = Vec::with_capacity(summary.trades.len());
    let mut drawdown_curve = Vec::with_capacity(summary.trades.len());
    let mut cumulative = 0.0_f64;
    let mut peak = 0.0_f64;
    let mut max_dd_abs = 0.0_f64;
    let mut max_dd_pct = 0.0_f64;

    for trade in &summary.trades {
        cumulative += trade.pnl;
        equity_curve.push(cumulative);

        if cumulative > peak {
            peak = cumulative;
        }
        let dd = peak - cumulative;
        drawdown_curve.push(dd);

        if dd > max_dd_abs {
            max_dd_abs = dd;
        }
        if peak > 0.0 {
            let dd_pct = dd / peak;
            if dd_pct > max_dd_pct {
                max_dd_pct = dd_pct;
            }
        }
    }

    // Calmar ratio (annualized return / max drawdown)
    let total_return = cumulative;
    let span_days = if summary.trades.len() >= 2 {
        let first = summary.trades.first().unwrap().entry_date;
        let last = summary.trades.last().unwrap().entry_date;
        (last - first).num_days().max(1) as f64
    } else {
        365.0
    };
    let annualized_return = total_return * (365.0 / span_days);
    let calmar_ratio = if max_dd_abs > 0.0 {
        annualized_return / max_dd_abs
    } else {
        0.0
    };

    // Profit factor
    let gross_profit: f64 = summary
        .trades
        .iter()
        .filter(|t| t.pnl > 0.0)
        .map(|t| t.pnl)
        .sum();
    let gross_loss: f64 = summary
        .trades
        .iter()
        .filter(|t| t.pnl < 0.0)
        .map(|t| t.pnl.abs())
        .sum();
    let profit_factor = if gross_loss > 0.0 {
        gross_profit / gross_loss
    } else if gross_profit > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    RiskMetrics {
        sharpe_ratio,
        sortino_ratio,
        max_drawdown_pct: max_dd_pct * 100.0,
        max_drawdown_abs: max_dd_abs,
        calmar_ratio,
        profit_factor,
        return_std,
        downside_std,
        equity_curve,
        drawdown_curve,
    }
}

// ── Volatility Analysis ─────────────────────────────────────────────────────

/// Volatility metrics for range sizing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilityMetrics {
    /// Average True Range (14-day) as percentage of price
    pub atr_14_pct: f64,
    /// Standard deviation of daily returns (annualized)
    pub annualized_vol: f64,
    /// Suggested range width based on historical volatility
    /// (2 standard deviations over the holding period)
    pub suggested_range_pct: f64,
    /// Current vs historical volatility ratio (>1 = elevated)
    pub vol_regime: f64,
    /// Daily returns standard deviation
    pub daily_vol: f64,
}

/// Calculate volatility metrics from price candles.
///
/// `holding_days` is used to scale volatility to the strategy timeframe.
pub fn calculate_volatility(candles: &[Candle], holding_days: u64) -> VolatilityMetrics {
    if candles.len() < 15 {
        return VolatilityMetrics {
            atr_14_pct: 0.0,
            annualized_vol: 0.0,
            suggested_range_pct: 10.0, // fallback
            vol_regime: 1.0,
            daily_vol: 0.0,
        };
    }

    // Daily returns
    let returns: Vec<f64> = candles
        .windows(2)
        .map(|w| (w[1].close / w[0].close).ln())
        .collect();

    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let daily_vol = variance.sqrt();
    let annualized_vol = daily_vol * (365.0_f64).sqrt();

    // ATR(14)
    let atr_values: Vec<f64> = candles
        .windows(2)
        .map(|w| {
            let high_low = w[1].high - w[1].low;
            let high_prev = (w[1].high - w[0].close).abs();
            let low_prev = (w[1].low - w[0].close).abs();
            high_low.max(high_prev).max(low_prev)
        })
        .collect();

    let atr_14 = if atr_values.len() >= 14 {
        atr_values[atr_values.len() - 14..].iter().sum::<f64>() / 14.0
    } else {
        atr_values.iter().sum::<f64>() / atr_values.len().max(1) as f64
    };
    let last_price = candles.last().map(|c| c.close).unwrap_or(1.0);
    let atr_14_pct = (atr_14 / last_price) * 100.0;

    // Suggested range: 2σ scaled to holding period
    let holding_vol = daily_vol * (holding_days as f64).sqrt();
    let suggested_range_pct = (holding_vol * 2.0 * 100.0).max(3.0).min(25.0);

    // Volatility regime: compare last 14 days to full history
    let recent_vol = if returns.len() >= 14 {
        let recent = &returns[returns.len() - 14..];
        let r_mean = recent.iter().sum::<f64>() / 14.0;
        let r_var = recent.iter().map(|r| (r - r_mean).powi(2)).sum::<f64>() / 13.0;
        r_var.sqrt()
    } else {
        daily_vol
    };
    let vol_regime = if daily_vol > 0.0 {
        recent_vol / daily_vol
    } else {
        1.0
    };

    VolatilityMetrics {
        atr_14_pct,
        annualized_vol: annualized_vol * 100.0,
        suggested_range_pct,
        vol_regime,
        daily_vol: daily_vol * 100.0,
    }
}

// ── Monte Carlo Simulation ──────────────────────────────────────────────────

/// Monte Carlo simulation results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    /// Number of simulations run
    pub num_simulations: usize,
    /// Number of trades per simulation
    pub trades_per_sim: usize,
    /// Median final PnL
    pub median_pnl: f64,
    /// 5th percentile PnL (worst case)
    pub pnl_5th: f64,
    /// 25th percentile PnL
    pub pnl_25th: f64,
    /// 75th percentile PnL
    pub pnl_75th: f64,
    /// 95th percentile PnL (best case)
    pub pnl_95th: f64,
    /// Probability of profit (% of simulations ending positive)
    pub prob_profit: f64,
    /// Maximum drawdown at 95th percentile
    pub max_drawdown_95th: f64,
    /// Expected Sharpe ratio median
    pub median_sharpe: f64,
}

/// Run Monte Carlo simulation by resampling trades from backtest results.
///
/// Uses a simple linear congruential generator for reproducibility.
pub fn monte_carlo(
    summary: &BacktestSummary,
    num_simulations: usize,
    trades_per_sim: usize,
) -> MonteCarloResult {
    if summary.trades.is_empty() {
        return MonteCarloResult {
            num_simulations,
            trades_per_sim,
            median_pnl: 0.0,
            pnl_5th: 0.0,
            pnl_25th: 0.0,
            pnl_75th: 0.0,
            pnl_95th: 0.0,
            prob_profit: 0.0,
            max_drawdown_95th: 0.0,
            median_sharpe: 0.0,
        };
    }

    let trade_pnls: Vec<f64> = summary.trades.iter().map(|t| t.pnl).collect();
    let trade_costs: Vec<f64> = summary.trades.iter().map(|t| t.entry_cost).collect();
    let n_trades = trade_pnls.len();

    let mut final_pnls = Vec::with_capacity(num_simulations);
    let mut max_drawdowns = Vec::with_capacity(num_simulations);
    let mut sharpes = Vec::with_capacity(num_simulations);

    // Simple LCG PRNG for reproducibility
    let mut seed: u64 = 42;
    let lcg_next = |s: &mut u64| -> usize {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*s >> 33) as usize) % n_trades
    };

    for _ in 0..num_simulations {
        let mut equity = 0.0_f64;
        let mut peak = 0.0_f64;
        let mut max_dd = 0.0_f64;
        let mut returns = Vec::with_capacity(trades_per_sim);

        for _ in 0..trades_per_sim {
            let idx = lcg_next(&mut seed);
            let pnl = trade_pnls[idx];
            let cost = trade_costs[idx];
            equity += pnl;
            returns.push(pnl / cost);

            if equity > peak {
                peak = equity;
            }
            let dd = peak - equity;
            if dd > max_dd {
                max_dd = dd;
            }
        }

        final_pnls.push(equity);
        max_drawdowns.push(max_dd);

        // Sharpe for this simulation
        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
        let std = var.sqrt();
        let sharpe = if std > 0.0 { mean / std } else { 0.0 };
        sharpes.push(sharpe);
    }

    final_pnls.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    max_drawdowns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let percentile = |sorted: &[f64], p: f64| -> f64 {
        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    };

    let prob_profit = final_pnls.iter().filter(|&&p| p > 0.0).count() as f64
        / num_simulations as f64
        * 100.0;

    MonteCarloResult {
        num_simulations,
        trades_per_sim,
        median_pnl: percentile(&final_pnls, 50.0),
        pnl_5th: percentile(&final_pnls, 5.0),
        pnl_25th: percentile(&final_pnls, 25.0),
        pnl_75th: percentile(&final_pnls, 75.0),
        pnl_95th: percentile(&final_pnls, 95.0),
        prob_profit,
        max_drawdown_95th: percentile(&max_drawdowns, 95.0),
        median_sharpe: percentile(&sharpes, 50.0),
    }
}

// ── Expected Value ──────────────────────────────────────────────────────────

/// Expected value analysis for a single trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedValue {
    /// EV per trade in absolute terms
    pub ev_per_trade: f64,
    /// EV as percentage of entry cost
    pub ev_pct: f64,
    /// Breakeven win rate needed for this payoff structure
    pub breakeven_win_rate: f64,
    /// Actual win rate observed
    pub actual_win_rate: f64,
    /// Edge over breakeven (positive = profitable)
    pub edge_over_breakeven: f64,
}

/// Calculate expected value from backtest outcomes.
pub fn expected_value(summary: &BacktestSummary) -> ExpectedValue {
    if summary.total_trades == 0 {
        return ExpectedValue {
            ev_per_trade: 0.0,
            ev_pct: 0.0,
            breakeven_win_rate: 0.0,
            actual_win_rate: 0.0,
            edge_over_breakeven: 0.0,
        };
    }

    let entry_cost = summary.trades.first().map(|t| t.entry_cost).unwrap_or(1.0);
    let profit = summary
        .trades
        .first()
        .map(|t| t.profit_in_rng)
        .unwrap_or(0.0);

    let p = summary.win_rate / 100.0;
    let ev_per_trade = p * profit + (1.0 - p) * (-entry_cost);
    let ev_pct = (ev_per_trade / entry_cost) * 100.0;

    // Breakeven: p * profit = (1-p) * cost → p = cost / (profit + cost)
    let breakeven_win_rate = (entry_cost / (profit + entry_cost)) * 100.0;
    let edge_over_breakeven = summary.win_rate - breakeven_win_rate;

    ExpectedValue {
        ev_per_trade,
        ev_pct,
        breakeven_win_rate,
        actual_win_rate: summary.win_rate,
        edge_over_breakeven,
    }
}

// ── Full Analytics Report ───────────────────────────────────────────────────

/// Complete analytics report combining all metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsReport {
    pub kelly: KellyResult,
    pub risk: RiskMetrics,
    pub volatility: VolatilityMetrics,
    pub monte_carlo: MonteCarloResult,
    pub expected_value: ExpectedValue,
}

/// Generate a full analytics report from backtest results and price data.
pub fn full_report(
    summary: &BacktestSummary,
    candles: &[Candle],
    holding_days: u64,
) -> AnalyticsReport {
    AnalyticsReport {
        kelly: kelly_criterion(summary),
        risk: calculate_risk_metrics(summary),
        volatility: calculate_volatility(candles, holding_days),
        monte_carlo: monte_carlo(summary, 10_000, summary.total_trades.max(50)),
        expected_value: expected_value(summary),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtesting::run_backtest;
    use chrono::{Duration, TimeZone, Utc};

    fn make_candles(prices: &[f64]) -> Vec<Candle> {
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let noise = ((i * 7 + 3) % 11) as f64 * 0.01;
                Candle {
                    timestamp: base + Duration::days(i as i64),
                    open: p * (1.0 - noise),
                    high: p * 1.02,
                    low: p * 0.98,
                    close: p,
                }
            })
            .collect()
    }

    #[test]
    fn test_kelly_all_wins() {
        let prices: Vec<f64> = vec![100.0; 30];
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);

        let kelly = kelly_criterion(&summary);
        assert!(kelly.win_prob > 0.99);
        assert!(kelly.full_kelly > 0.0);
        assert!(kelly.half_kelly > 0.0);
        assert!(kelly.half_kelly < kelly.full_kelly);
        assert!(kelly.quarter_kelly < kelly.half_kelly);
        println!("Kelly (all wins): full={:.4} half={:.4} edge={:.4}",
            kelly.full_kelly, kelly.half_kelly, kelly.edge);
    }

    #[test]
    fn test_kelly_mixed_outcomes() {
        let mut prices: Vec<f64> = vec![100.0; 15];
        prices.extend(vec![80.0; 15]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);

        let kelly = kelly_criterion(&summary);
        assert!(kelly.win_prob > 0.0 && kelly.win_prob < 1.0);
        println!(
            "Kelly (mixed): full={:.4} half={:.4} edge={:.4} win_p={:.2}",
            kelly.full_kelly, kelly.half_kelly, kelly.edge, kelly.win_prob
        );
    }

    #[test]
    fn test_risk_metrics_basic() {
        let mut prices: Vec<f64> = vec![100.0; 20];
        prices.extend(vec![85.0; 20]);
        prices.extend(vec![100.0; 20]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);

        let risk = calculate_risk_metrics(&summary);
        assert!(risk.max_drawdown_abs >= 0.0);
        assert!(!risk.equity_curve.is_empty());
        assert_eq!(risk.equity_curve.len(), risk.drawdown_curve.len());

        if summary.losing_trades > 0 {
            assert!(risk.profit_factor > 0.0);
        }

        println!(
            "Risk: sharpe={:.2} sortino={:.2} max_dd={:.1}% profit_factor={:.2}",
            risk.sharpe_ratio, risk.sortino_ratio, risk.max_drawdown_pct, risk.profit_factor
        );
    }

    #[test]
    fn test_volatility_metrics() {
        let mut prices = Vec::new();
        for i in 0..90 {
            let noise = ((i * 13 + 7) % 19) as f64 * 0.005 - 0.045;
            prices.push(50000.0 * (1.0 + noise));
        }
        let candles = make_candles(&prices);

        let vol = calculate_volatility(&candles, 7);
        assert!(vol.daily_vol > 0.0);
        assert!(vol.annualized_vol > 0.0);
        assert!(vol.suggested_range_pct > 0.0);
        assert!(vol.atr_14_pct > 0.0);

        println!(
            "Volatility: daily={:.2}% annual={:.1}% suggested_range={:.1}% ATR14={:.2}%",
            vol.daily_vol, vol.annualized_vol, vol.suggested_range_pct, vol.atr_14_pct
        );
    }

    #[test]
    fn test_monte_carlo() {
        let mut prices: Vec<f64> = vec![100.0; 20];
        prices.extend(vec![85.0; 10]);
        prices.extend(vec![100.0; 20]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);

        let mc = monte_carlo(&summary, 1000, 50);
        assert_eq!(mc.num_simulations, 1000);
        assert_eq!(mc.trades_per_sim, 50);
        assert!(mc.pnl_5th <= mc.pnl_25th);
        assert!(mc.pnl_25th <= mc.median_pnl);
        assert!(mc.median_pnl <= mc.pnl_75th);
        assert!(mc.pnl_75th <= mc.pnl_95th);
        assert!(mc.prob_profit >= 0.0 && mc.prob_profit <= 100.0);

        println!(
            "Monte Carlo: median={:.2} [5th={:.2}, 95th={:.2}] P(profit)={:.1}%",
            mc.median_pnl, mc.pnl_5th, mc.pnl_95th, mc.prob_profit
        );
    }

    #[test]
    fn test_expected_value() {
        let mut prices: Vec<f64> = vec![100.0; 15];
        prices.extend(vec![80.0; 15]);
        let candles = make_candles(&prices);
        let summary = run_backtest(&candles, 0.90, 1.10, 7, 0.6, 0.7);

        let ev = expected_value(&summary);
        assert!(ev.breakeven_win_rate > 0.0 && ev.breakeven_win_rate < 100.0);
        // For cost=0.9, profit=1.1: breakeven = 0.9 / (1.1 + 0.9) = 45%
        assert!((ev.breakeven_win_rate - 45.0).abs() < 1.0);

        println!(
            "EV: per_trade={:.4} ev%={:.2}% breakeven={:.1}% actual={:.1}% edge={:.1}pp",
            ev.ev_per_trade, ev.ev_pct, ev.breakeven_win_rate, ev.actual_win_rate, ev.edge_over_breakeven
        );
    }

    #[test]
    fn test_full_report_with_embedded_data() {
        // Use embedded historical data for a comprehensive test
        let candles = crate::historical_data::generate_embedded_candles();

        let summary = run_backtest(&candles, 0.92, 1.08, 7, 0.60, 0.70);
        let report = full_report(&summary, &candles, 7);

        println!("\n{}", "=".repeat(60));
        println!("  FULL ANALYTICS REPORT (Embedded BTC Data, ±8%, 7d)");
        println!("{}", "=".repeat(60));
        println!("  Trades: {} | Win Rate: {:.1}%", summary.total_trades, summary.win_rate);
        println!("  Total PnL: {:.4}", summary.total_pnl);
        println!("\n  -- Kelly Criterion --");
        println!("  Full Kelly: {:.1}% | Half Kelly: {:.1}%",
            report.kelly.full_kelly * 100.0, report.kelly.half_kelly * 100.0);
        println!("  Edge: {:.4} | Win/Loss ratio: {:.2}", report.kelly.edge, report.kelly.win_loss_ratio);
        println!("\n  -- Risk Metrics --");
        println!("  Sharpe: {:.2} | Sortino: {:.2}", report.risk.sharpe_ratio, report.risk.sortino_ratio);
        println!("  Max DD: {:.1}% ({:.2} abs) | Profit Factor: {:.2}",
            report.risk.max_drawdown_pct, report.risk.max_drawdown_abs, report.risk.profit_factor);
        println!("  Calmar: {:.2}", report.risk.calmar_ratio);
        println!("\n  -- Volatility --");
        println!("  Daily: {:.2}% | Annual: {:.1}%", report.volatility.daily_vol, report.volatility.annualized_vol);
        println!("  Suggested range: ±{:.1}% | Vol regime: {:.2}x",
            report.volatility.suggested_range_pct, report.volatility.vol_regime);
        println!("\n  -- Monte Carlo (10k sims) --");
        println!("  Median PnL: {:.2} | P(profit): {:.1}%", report.monte_carlo.median_pnl, report.monte_carlo.prob_profit);
        println!("  5th-95th: [{:.2}, {:.2}]", report.monte_carlo.pnl_5th, report.monte_carlo.pnl_95th);
        println!("  Max DD 95th: {:.2}", report.monte_carlo.max_drawdown_95th);
        println!("\n  -- Expected Value --");
        println!("  EV/trade: {:.4} ({:.2}%)", report.expected_value.ev_per_trade, report.expected_value.ev_pct);
        println!("  Breakeven WR: {:.1}% | Actual: {:.1}% | Edge: {:.1}pp",
            report.expected_value.breakeven_win_rate, report.expected_value.actual_win_rate,
            report.expected_value.edge_over_breakeven);

        // Assertions
        assert!(summary.total_trades > 500);
        assert!(report.kelly.full_kelly >= 0.0);
        assert!(report.risk.equity_curve.len() == summary.total_trades);
        assert!(report.expected_value.breakeven_win_rate > 0.0);
    }
}
