mod analytics;
mod backtesting;
mod dashboard;
mod dry_run;
mod historical_data;
mod polymarket_ws;
mod scanner;
mod types;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use dashboard::AppState;
use scanner::{calculate_structure, fetch_historical_btc, find_best_pairs, get_btc_price};
use types::{OutputPair, ScanResult};

// ── CLI definition ─────────────────────────────────────────────────────────────

/// PolyDelta — Delta-neutral BTC range strategy finder for Polymarket (Rust edition)
///
/// SECURITY NOTE: The original polydelta Python project (st1ne/polydelta) contained
/// hardcoded referral affiliate links (?via=SolSt1ne) in all generated market URLs.
/// Those links have been removed in this implementation. No other backdoors or
/// malicious code patterns were identified in the original source.
#[derive(Parser, Debug)]
#[command(
    name = "polydelta",
    version,
    about = "Delta-neutral BTC range strategy finder for Polymarket",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Scan Polymarket and display results in the web dashboard
    Scan {
        /// Enable dry-run mode — simulate orders without execution
        #[arg(long, short = 'd')]
        dry_run: bool,

        /// Timeframe to scan: "week" or "month"
        #[arg(long, default_value = "week")]
        timeframe: String,

        /// Port for the web dashboard
        #[arg(long, default_value_t = 8080)]
        port: u16,

        /// Capital per pair in USDC (for cost breakdown display)
        #[arg(long, default_value_t = 100.0)]
        balance: f64,

        /// Enable WebSocket live price feed from Polymarket CLOB
        #[arg(long)]
        live: bool,

        /// Scan interval in seconds (0 = scan once and exit)
        #[arg(long, default_value_t = 300)]
        interval: u64,
    },

    /// Run a historical backtest of the strategy
    Backtest {
        /// Lower price bound as % of BTC spot (e.g. 92 = 92%)
        #[arg(long, default_value_t = 92.0)]
        low_pct: f64,

        /// Upper price bound as % of BTC spot (e.g. 108 = 108%)
        #[arg(long, default_value_t = 108.0)]
        high_pct: f64,

        /// Trade duration in days
        #[arg(long, default_value_t = 7)]
        duration_days: i64,

        /// Assumed YES-leg entry price (0..1)
        #[arg(long, default_value_t = 0.55)]
        yes_price_low: f64,

        /// Assumed HIGH-leg YES entry price (0..1)
        #[arg(long, default_value_t = 0.65)]
        yes_price_high: f64,

        /// Number of days of historical BTC data to fetch (ignored with --offline or --csv)
        #[arg(long, default_value_t = 90)]
        history_days: u32,

        /// Use embedded historical data (~850 days, Jan 2023–Apr 2025) instead of API
        #[arg(long)]
        offline: bool,

        /// Load BTC price data from a CSV file (format: date,open,high,low,close)
        #[arg(long)]
        csv: Option<String>,

        /// Stop-loss: close trade if BTC moves this % beyond the range thresholds.
        /// E.g. 5 = close if BTC drops 5% below low or rises 5% above high.
        /// Default: 5. Use --stop-loss 0 to disable.
        #[arg(long, default_value = "5")]
        stop_loss: Option<f64>,

        /// Take-profit: close early when BTC is within this fraction of the range center.
        /// E.g. 80 = take profit when 80% confident (past 50% of holding period).
        /// Default: 80. Use --take-profit 0 to disable.
        #[arg(long, default_value = "80")]
        take_profit: Option<f64>,

        /// Entry interval: "daily", "weekly", or "monthly"
        #[arg(long, default_value = "weekly")]
        interval: String,
    },
}

// ── Entry point ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let http = reqwest::Client::builder()
        .user_agent("polydelta-rs/0.1")
        .build()?;

    match cli.command {
        Command::Scan {
            dry_run,
            timeframe,
            port,
            balance,
            live,
            interval,
        } => {
            run_scan(http, dry_run, timeframe, port, balance, live, interval).await?;
        }
        Command::Backtest {
            low_pct,
            high_pct,
            duration_days,
            yes_price_low,
            yes_price_high,
            history_days,
            offline,
            csv,
            stop_loss,
            take_profit,
            interval,
        } => {
            // Convert % to ratio; 0 disables the feature
            let sl = stop_loss.filter(|&v| v > 0.0).map(|v| v / 100.0);
            let tp = take_profit.filter(|&v| v > 0.0).map(|v| v / 100.0);
            run_backtest_cli(
                http,
                low_pct / 100.0,
                high_pct / 100.0,
                duration_days,
                yes_price_low,
                yes_price_high,
                history_days,
                offline,
                csv,
                sl,
                tp,
                interval,
            )
            .await?;
        }
    }

    Ok(())
}

// ── Scan command ───────────────────────────────────────────────────────────────

async fn run_scan(
    http: reqwest::Client,
    dry_run: bool,
    timeframe: String,
    port: u16,
    balance: f64,
    live: bool,
    interval: u64,
) -> Result<()> {
    if dry_run {
        warn!("🟡 DRY-RUN MODE enabled — no real orders will be placed");
    }

    let state = AppState::new(http.clone());
    let shared_result = state.scan_result.clone();

    // Start dashboard server in background
    let server_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = dashboard::start_server(server_state, port).await {
            error!("Dashboard server error: {e}");
        }
    });

    // Optionally start WebSocket price feed
    let (ws_tx, mut ws_rx) = broadcast::channel::<polymarket_ws::PriceUpdate>(256);

    if live {
        info!("📡 Starting Polymarket WebSocket price feed...");
        let tx = ws_tx.clone();
        tokio::spawn(async move {
            // We subscribe to all markets after the first scan — start with empty list.
            // In a production system this would be updated dynamically.
            if let Err(e) = polymarket_ws::start_ws_listener(vec![], tx).await {
                error!("WebSocket listener error: {e}");
            }
        });

        // Log incoming WS updates (non-blocking)
        tokio::spawn(async move {
            loop {
                match ws_rx.recv().await {
                    Ok(update) => {
                        info!(
                            "💹 [WS] token={} price={:.4} side={}",
                            &update.token_id[..8.min(update.token_id.len())],
                            update.price,
                            update.side
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket receiver lagged by {n} messages");
                    }
                }
            }
        });
    }

    // Scan loop
    loop {
        info!("🔍 Scanning Polymarket... (timeframe={})", timeframe);

        let btc_price = match get_btc_price(&http).await {
            Ok(p) => {
                info!("₿  BTC/USD: ${p:.0}");
                p
            }
            Err(e) => {
                error!("Failed to fetch BTC price: {e}");
                if interval == 0 {
                    return Err(e);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                continue;
            }
        };

        let pairs = match find_best_pairs(&http, btc_price, &timeframe).await {
            Ok(p) => p,
            Err(e) => {
                error!("Scan error: {e}");
                if interval == 0 {
                    return Err(e);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                continue;
            }
        };

        info!("✅ Found {} delta-neutral pairs", pairs.len());

        let output_pairs: Vec<OutputPair> = pairs
            .iter()
            .map(|p| {
                let low = &p.low_market;
                let high = &p.high_market;
                let calc = calculate_structure(p.yes_price_low, p.yes_price_high, balance);

                let low_pct = (low.threshold / btc_price - 1.0) * 100.0;
                let high_pct = (high.threshold / btc_price - 1.0) * 100.0;

                // In dry-run mode: print the simulated orders to stdout
                if dry_run {
                    let no_token = low
                        .no_token_id
                        .as_deref()
                        .unwrap_or("unknown_no_token");
                    const THOUSAND: f64 = 1000.0;
                    let label = format!("BTC ${:.0}k–${:.0}k", low.threshold / THOUSAND, high.threshold / THOUSAND);
                    dry_run::simulate_pair_entry(
                        &label,
                        &low.yes_token_id,
                        no_token,
                        p.yes_price_low,
                        calc.no_price,
                        balance,
                    );
                }

                let expiry_date = low.end_date.format("%d.%m").to_string();

                OutputPair {
                    low_threshold: low.threshold,
                    high_threshold: high.threshold,
                    low_question: low.question.clone(),
                    high_question: high.question.clone(),
                    low_url: format!("https://polymarket.com/event/{}", low.slug),
                    high_url: format!("https://polymarket.com/event/{}", high.slug),
                    expiry: low.end_date.to_rfc3339(),
                    expiry_date,
                    days_until: low.days_until,
                    yes_price_low: round4(p.yes_price_low),
                    yes_price_high: round4(p.yes_price_high),
                    no_price: round4(calc.no_price),
                    cost_per_unit: round4(calc.cost_per_unit),
                    cost_low: round2(calc.cost_low),
                    cost_high: round2(calc.cost_high),
                    profit_in_rng: round4(calc.profit_in_rng),
                    profit_pct: round2(calc.profit_pct),
                    rr_reward: round4(calc.rr_reward),
                    low_pct: round1(low_pct),
                    high_pct: round1(high_pct),
                }
            })
            .collect();

        let result = ScanResult {
            generated_at: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            btc_price,
            pairs: output_pairs,
            dry_run,
        };

        *shared_result.write().await = Some(result);
        info!("📊 Dashboard data updated");

        if interval == 0 {
            info!("🌐 Dashboard: http://127.0.0.1:{port}/");
            info!("   Press Ctrl+C to stop.");
            // Keep the server running
            tokio::signal::ctrl_c().await?;
            break;
        }

        info!("⏳ Next scan in {interval}s...");
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
    }

    Ok(())
}

// ── Backtest CLI command ───────────────────────────────────────────────────────

async fn run_backtest_cli(
    http: reqwest::Client,
    low_ratio: f64,
    high_ratio: f64,
    duration_days: i64,
    yes_price_low: f64,
    yes_price_high: f64,
    history_days: u32,
    offline: bool,
    csv_path: Option<String>,
    stop_loss_pct: Option<f64>,
    take_profit_pct: Option<f64>,
    entry_interval: String,
) -> Result<()> {
    info!(
        "📈 Running backtest: range=[{:.0}%–{:.0}%], duration={}d, interval={}",
        low_ratio * 100.0,
        high_ratio * 100.0,
        duration_days,
        entry_interval,
    );

    let candles = if let Some(ref path) = csv_path {
        info!("Loading data from CSV: {path}");
        historical_data::load_candles_from_csv(std::path::Path::new(path))?
    } else if offline {
        info!("Using embedded historical data");
        historical_data::generate_embedded_candles()
    } else {
        info!("Fetching {history_days}d from CoinGecko...");
        fetch_historical_btc(&http, history_days).await?
    };
    info!("Loaded {} daily candles", candles.len());

    let config = types::BacktestConfig {
        low_ratio,
        high_ratio,
        duration_days,
        yes_price_low,
        yes_price_high,
        stop_loss_pct,
        take_profit_pct,
        entry_interval: entry_interval.clone(),
    };

    let summary = backtesting::run_backtest_advanced(&candles, &config);

    println!("\n{}", "=".repeat(60));
    println!("  BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    println!("  Total trades    : {}", summary.total_trades);
    println!("  Winning trades  : {} ({:.1}%)", summary.winning_trades, summary.win_rate);
    println!("  Losing trades   : {}", summary.losing_trades);
    println!("  Total PnL       : {:.4}", summary.total_pnl);
    println!("  Avg Profit %    : {:.2}%", summary.avg_profit_pct);
    if summary.stopped_out > 0 {
        println!("  Stopped out     : {}", summary.stopped_out);
    }
    if summary.took_profit > 0 {
        println!("  Took profit     : {}", summary.took_profit);
    }
    println!("  Entry interval  : {}", entry_interval);
    println!("{}", "=".repeat(60));

    if !summary.trades.is_empty() {
        println!("\n  Recent trades (last 10):");
        println!("  {:<12} {:<12} {:<22} {:<10} {:<10} {:<8}", "Entry", "Expiry", "Range", "BTC Exp.", "PnL", "Result");
        println!("  {}", "-".repeat(78));
        for t in summary.trades.iter().rev().take(10) {
            println!(
                "  {:<12} {:<12} ${:<10.0}–${:<10.0} {:<10.0} {:<+10.4} {}",
                t.entry_date.format("%Y-%m-%d"),
                t.expiry_date.format("%Y-%m-%d"),
                t.low_threshold,
                t.high_threshold,
                t.btc_at_expiry,
                t.pnl,
                if t.won { "✓ WIN" } else { "✗ LOSS" },
            );
        }
    }

    // Advanced analytics
    let report = analytics::full_report(&summary, &candles, duration_days as u64);

    println!("\n{}", "=".repeat(60));
    println!("  ADVANCED ANALYTICS");
    println!("{}", "=".repeat(60));

    println!("\n  Kelly Criterion (position sizing):");
    println!("    Full Kelly    : {:.1}% of bankroll", report.kelly.full_kelly * 100.0);
    println!("    Half Kelly    : {:.1}% (recommended)", report.kelly.half_kelly * 100.0);
    println!("    Quarter Kelly : {:.1}% (conservative)", report.kelly.quarter_kelly * 100.0);
    println!("    Edge          : {:.4}", report.kelly.edge);
    println!("    Win/Loss ratio: {:.2}", report.kelly.win_loss_ratio);

    println!("\n  Risk-Adjusted Returns:");
    println!("    Sharpe ratio  : {:.2}", report.risk.sharpe_ratio);
    println!("    Sortino ratio : {:.2}", report.risk.sortino_ratio);
    println!("    Max drawdown  : {:.1}% ({:.2} abs)", report.risk.max_drawdown_pct, report.risk.max_drawdown_abs);
    println!("    Calmar ratio  : {:.2}", report.risk.calmar_ratio);
    println!("    Profit factor : {:.2}", report.risk.profit_factor);

    println!("\n  Volatility Analysis:");
    println!("    Daily vol     : {:.2}%", report.volatility.daily_vol);
    println!("    Annual vol    : {:.1}%", report.volatility.annualized_vol);
    println!("    ATR(14)       : {:.2}%", report.volatility.atr_14_pct);
    println!("    Suggested rng : +/-{:.1}%", report.volatility.suggested_range_pct);
    println!("    Vol regime    : {:.2}x (>1 = elevated)", report.volatility.vol_regime);

    println!("\n  Monte Carlo (10k simulations):");
    println!("    Median PnL    : {:.2}", report.monte_carlo.median_pnl);
    println!("    5th–95th pct  : [{:.2}, {:.2}]", report.monte_carlo.pnl_5th, report.monte_carlo.pnl_95th);
    println!("    P(profit)     : {:.1}%", report.monte_carlo.prob_profit);
    println!("    Max DD (95th) : {:.2}", report.monte_carlo.max_drawdown_95th);

    println!("\n  Expected Value:");
    println!("    EV per trade  : {:.4} ({:.2}%)", report.expected_value.ev_per_trade, report.expected_value.ev_pct);
    println!("    Breakeven WR  : {:.1}%", report.expected_value.breakeven_win_rate);
    println!("    Actual WR     : {:.1}%", report.expected_value.actual_win_rate);
    println!("    Edge over BE  : {:+.1}pp", report.expected_value.edge_over_breakeven);
    println!("{}", "=".repeat(60));

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn round4(x: f64) -> f64 { (x * 10000.0).round() / 10000.0 }
fn round2(x: f64) -> f64 { (x * 100.0).round() / 100.0 }
fn round1(x: f64) -> f64 { (x * 10.0).round() / 10.0 }
