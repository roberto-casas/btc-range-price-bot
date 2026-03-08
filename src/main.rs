mod ai_advisor;
mod analytics;
mod backtesting;
mod config;
mod dashboard;
mod db;
mod dry_run;
mod historical_data;
mod polymarket_ws;
mod scanner;
mod types;

use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use config::load_config;
use dashboard::AppState;
use db::Db;
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
    /// Path to JSON config file with default bot parameters
    #[arg(long, global = true, default_value = "bot-config.json")]
    config: String,

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
        #[arg(long)]
        timeframe: Option<String>,

        /// Host address to bind the web dashboard (e.g. 0.0.0.0 for all interfaces)
        #[arg(long)]
        host: Option<String>,

        /// Port for the web dashboard
        #[arg(long)]
        port: Option<u16>,

        /// Capital per pair in USDC (for cost breakdown display)
        #[arg(long)]
        balance: Option<f64>,

        /// Enable WebSocket live price feed from Polymarket CLOB
        #[arg(long)]
        live: bool,

        /// Scan interval in seconds (0 = scan once and exit)
        #[arg(long)]
        interval: Option<u64>,

        /// Path to the SQLite database for persistent dry-run state
        #[arg(long, default_value = db::DEFAULT_DB_PATH)]
        db_path: String,
    },

    /// Run a historical backtest of the strategy
    Backtest {
        /// Lower price bound as % of BTC spot (e.g. 92 = 92%)
        #[arg(long)]
        low_pct: Option<f64>,

        /// Upper price bound as % of BTC spot (e.g. 108 = 108%)
        #[arg(long)]
        high_pct: Option<f64>,

        /// Trade duration in days
        #[arg(long)]
        duration_days: Option<i64>,

        /// Assumed YES-leg entry price (0..1)
        #[arg(long)]
        yes_price_low: Option<f64>,

        /// Assumed HIGH-leg YES entry price (0..1)
        #[arg(long)]
        yes_price_high: Option<f64>,

        /// Number of days of historical BTC data to fetch (ignored with --offline or --csv)
        #[arg(long)]
        history_days: Option<u32>,

        /// Use embedded historical data (~850 days, Jan 2023–Apr 2025) instead of API
        #[arg(long)]
        offline: bool,

        /// Load BTC price data from a CSV file (format: date,open,high,low,close)
        #[arg(long)]
        csv: Option<String>,

        /// Stop-loss: close trade if BTC moves this % beyond the range thresholds.
        /// E.g. 5 = close if BTC drops 5% below low or rises 5% above high.
        /// Default: 5. Use --stop-loss 0 to disable.
        #[arg(long)]
        stop_loss: Option<f64>,

        /// Take-profit: close early when BTC is within this fraction of the range center.
        /// E.g. 80 = take profit when 80% confident (past 50% of holding period).
        /// Default: 80. Use --take-profit 0 to disable.
        #[arg(long)]
        take_profit: Option<f64>,

        /// Entry interval: "daily", "weekly", or "monthly"
        #[arg(long)]
        interval: Option<String>,

        /// Spread per leg as percentage (e.g. 2 = 2% spread on each leg).
        /// Simulates bid/ask spread cost. Default: 0.
        #[arg(long)]
        spread: Option<f64>,

        /// Platform fee as percentage of trade value (e.g. 1 = 1% fee).
        /// Applied on entry and exit. Default: 0.
        #[arg(long)]
        fee: Option<f64>,

        /// Slippage as percentage (e.g. 0.5 = 0.5% price impact).
        /// Applied to both legs at entry. Default: 0.
        #[arg(long)]
        slippage: Option<f64>,
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
    let config_path = std::path::Path::new(&cli.config);
    let app_config = load_config(config_path)?;

    let http = reqwest::Client::builder()
        .user_agent("polydelta-rs/0.1")
        .build()?;

    match cli.command {
        Command::Scan {
            dry_run,
            timeframe,
            host,
            port,
            balance,
            live,
            interval,
            db_path,
        } => {
            let scan_defaults = app_config.scan;
            let resolved_timeframe = timeframe.unwrap_or(scan_defaults.timeframe);
            let resolved_host = host.unwrap_or(scan_defaults.host);
            let resolved_port = port.unwrap_or(scan_defaults.port);
            let resolved_balance = balance.unwrap_or(scan_defaults.balance);
            let resolved_interval = interval.unwrap_or(scan_defaults.interval);
            let resolved_dry_run = dry_run || scan_defaults.dry_run;
            let resolved_live = live || scan_defaults.live;

            run_scan(
                http,
                resolved_dry_run,
                resolved_timeframe,
                resolved_host,
                resolved_port,
                resolved_balance,
                resolved_live,
                resolved_interval,
                db_path,
            )
            .await?;
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
            spread,
            fee,
            slippage,
        } => {
            let backtest_defaults = app_config.backtest;
            let resolved_low_pct = low_pct.unwrap_or(backtest_defaults.low_pct);
            let resolved_high_pct = high_pct.unwrap_or(backtest_defaults.high_pct);
            let resolved_duration_days = duration_days.unwrap_or(backtest_defaults.duration_days);
            let resolved_yes_price_low = yes_price_low.unwrap_or(backtest_defaults.yes_price_low);
            let resolved_yes_price_high =
                yes_price_high.unwrap_or(backtest_defaults.yes_price_high);
            let resolved_history_days = history_days.unwrap_or(backtest_defaults.history_days);
            let resolved_offline = offline || backtest_defaults.offline;
            let resolved_interval = interval.unwrap_or(backtest_defaults.interval);

            // Convert % to ratio; 0 disables the feature
            let stop_loss_value = stop_loss.unwrap_or(backtest_defaults.stop_loss);
            let take_profit_value = take_profit.unwrap_or(backtest_defaults.take_profit);
            let sl = (stop_loss_value > 0.0).then(|| stop_loss_value / 100.0);
            let tp = (take_profit_value > 0.0).then(|| take_profit_value / 100.0);

            // Convert cost % to fractions (use config defaults if not provided via CLI)
            let spread_frac = Some(spread.unwrap_or(backtest_defaults.spread) / 100.0);
            let fee_frac = Some(fee.unwrap_or(backtest_defaults.fee) / 100.0);
            let slippage_frac = Some(slippage.unwrap_or(backtest_defaults.slippage) / 100.0);

            run_backtest_cli(
                http,
                resolved_low_pct / 100.0,
                resolved_high_pct / 100.0,
                resolved_duration_days,
                resolved_yes_price_low,
                resolved_yes_price_high,
                resolved_history_days,
                resolved_offline,
                csv,
                sl,
                tp,
                resolved_interval,
                spread_frac,
                fee_frac,
                slippage_frac,
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
    host: String,
    port: u16,
    balance: f64,
    live: bool,
    interval: u64,
    db_path: String,
) -> Result<()> {
    if dry_run {
        warn!("DRY-RUN MODE enabled — no real orders will be placed");
    }

    // Open (or create) the SQLite database for persistent dry-run state
    let database = Arc::new(Db::open(std::path::Path::new(&db_path))?);

    if dry_run {
        // Report persisted state from previous runs
        let orders = database.get_all_orders()?;
        let open_count = orders.iter().filter(|o| o.status == "open").count();
        let closed_count = orders.iter().filter(|o| o.status != "open").count();
        info!(
            "DB loaded: {} total orders ({} open, {} settled)",
            orders.len(),
            open_count,
            closed_count
        );
    }

    let state = AppState::new(http.clone(), database.clone(), dry_run);
    let shared_result = state.scan_result.clone();

    // Start dashboard server in background
    let server_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = dashboard::start_server(server_state, &host, port).await {
            error!("Dashboard server error: {e}");
        }
    });

    // Optionally start WebSocket price feed
    let (ws_tx, mut ws_rx) = broadcast::channel::<polymarket_ws::PriceUpdate>(256);

    if live {
        info!("Starting Polymarket WebSocket price feed...");
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
                            "[WS] token={} price={:.4} side={}",
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

    // Check for AI advisor availability
    if ai_advisor::is_available() {
        info!("OpenAI AI advisor is enabled (OPENAI_API_KEY detected)");
    }

    // Scan loop
    loop {
        info!("Scanning Polymarket... (timeframe={})", timeframe);

        let btc_price = match get_btc_price(&http).await {
            Ok(p) => {
                info!("BTC/USD: ${p:.0}");
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

        // Record price in DB
        if let Err(e) = database.insert_price(btc_price) {
            warn!("Failed to record BTC price in DB: {e}");
        }

        // Settle any expired orders
        if dry_run {
            match database.settle_expired_orders(btc_price) {
                Ok(n) if n > 0 => info!("[DRY-RUN] Settled {n} expired position(s)"),
                Err(e) => warn!("Failed to settle expired orders: {e}"),
                _ => {}
            }
        }

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

        info!("Found {} delta-neutral pairs", pairs.len());

        // Collect recent prices for AI advisor context
        let recent_prices: Vec<f64> = database
            .get_recent_prices(14)
            .unwrap_or_default()
            .iter()
            .map(|p| p.btc_price)
            .collect();

        let output_pairs: Vec<OutputPair> = pairs
            .iter()
            .map(|p| {
                let low = &p.low_market;
                let high = &p.high_market;
                let calc = calculate_structure(p.yes_price_low, p.yes_price_high, balance);

                let low_pct = (low.threshold / btc_price - 1.0) * 100.0;
                let high_pct = (high.threshold / btc_price - 1.0) * 100.0;

                // In dry-run mode: simulate and persist the orders
                if dry_run {
                    let no_token = low.no_token_id.as_deref().unwrap_or("unknown_no_token");
                    const THOUSAND: f64 = 1000.0;
                    let label = format!(
                        "BTC ${:.0}k–${:.0}k",
                        low.threshold / THOUSAND,
                        high.threshold / THOUSAND
                    );
                    let expiry = low.end_date.to_rfc3339();
                    dry_run::simulate_pair_entry_persistent(
                        &database,
                        &label,
                        &low.yes_token_id,
                        no_token,
                        p.yes_price_low,
                        calc.no_price,
                        balance,
                        btc_price,
                        &expiry,
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

        // Run AI advisor if available (for the best pair)
        let ai_assessment = if ai_advisor::is_available() && !output_pairs.is_empty() {
            let best = &output_pairs[0];
            let ctx = ai_advisor::AdvisorContext {
                btc_price,
                proposed_low_threshold: best.low_threshold,
                proposed_high_threshold: best.high_threshold,
                low_pct_from_spot: best.low_pct,
                high_pct_from_spot: best.high_pct,
                days_until_expiry: best.days_until,
                profit_pct: best.profit_pct,
                recent_prices: recent_prices.clone(),
                daily_volatility_pct: None,
                atr_14_pct: None,
            };
            Some(ai_advisor::assess_risk(&http, &ctx).await)
        } else {
            None
        };

        // Record evaluation and decision in DB
        {
            let (pair_label, low_thresh, high_thresh, profit, days) = if !output_pairs.is_empty() {
                let best = &output_pairs[0];
                const THOUSAND: f64 = 1000.0;
                let label = format!(
                    "BTC ${:.0}k–${:.0}k",
                    best.low_threshold / THOUSAND,
                    best.high_threshold / THOUSAND
                );
                (label, best.low_threshold, best.high_threshold, best.profit_pct, best.days_until)
            } else {
                (String::new(), 0.0, 0.0, 0.0, 0)
            };

            let (risk_level, confidence, skip, reasoning, factors, low_adj, high_adj) =
                if let Some(ref ai) = ai_assessment {
                    (
                        ai.risk_level.clone(),
                        ai.confidence,
                        ai.skip_trade,
                        ai.reasoning.clone(),
                        serde_json::to_string(&ai.risk_factors).unwrap_or_else(|_| "[]".to_string()),
                        ai.suggested_low_adjust_pct,
                        ai.suggested_high_adjust_pct,
                    )
                } else {
                    ("n/a".to_string(), 0.0, false, String::new(), "[]".to_string(), 0.0, 0.0)
                };

            let decision = if output_pairs.is_empty() {
                "no_pairs"
            } else if skip {
                "skipped_ai"
            } else if dry_run {
                "entered"
            } else {
                "scanned"
            };

            let eval = db::DbEvaluation {
                id: 0,
                created_at: Utc::now().to_rfc3339(),
                btc_price,
                pair_label,
                low_threshold: low_thresh,
                high_threshold: high_thresh,
                profit_pct: profit,
                days_until: days,
                pairs_found: output_pairs.len() as i64,
                risk_level,
                confidence,
                skip_trade: skip,
                reasoning,
                risk_factors: factors,
                suggested_low_adj: low_adj,
                suggested_high_adj: high_adj,
                decision: decision.to_string(),
            };
            if let Err(e) = database.insert_evaluation(&eval) {
                warn!("Failed to record evaluation: {e}");
            }
        }

        // Save portfolio snapshot in dry-run mode
        if dry_run {
            if let Ok(snap) = database.compute_snapshot(btc_price, balance * 10.0) {
                if let Err(e) = database.insert_snapshot(&snap) {
                    warn!("Failed to save portfolio snapshot: {e}");
                }
                info!(
                    "[DRY-RUN] Portfolio: balance={:.2} invested={:.2} pnl={:.2} open={} won={} lost={}",
                    snap.balance, snap.total_invested, snap.total_pnl,
                    snap.open_positions, snap.win_count, snap.loss_count
                );
            }
        }

        let result = ScanResult {
            generated_at: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            btc_price,
            pairs: output_pairs,
            dry_run,
            ai_assessment,
        };

        *shared_result.write().await = Some(result);
        info!("Dashboard data updated");

        if interval == 0 {
            info!("Dashboard: http://127.0.0.1:{port}/");
            info!("   Press Ctrl+C to stop.");
            // Keep the server running
            tokio::signal::ctrl_c().await?;
            break;
        }

        info!("Next scan in {interval}s...");
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
    spread_per_leg: Option<f64>,
    fee_pct: Option<f64>,
    slippage_pct: Option<f64>,
) -> Result<()> {
    info!(
        "Running backtest: range=[{:.0}%–{:.0}%], duration={}d, interval={}",
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
        spread_per_leg,
        fee_pct,
        slippage_pct,
    };

    let summary = backtesting::run_backtest_advanced(&candles, &config);

    // Compute effective costs for display
    let no_price = 1.0 - yes_price_high;
    let spread_val = spread_per_leg.unwrap_or(0.0);
    let slip_val = slippage_pct.unwrap_or(0.0);
    let fee_val = fee_pct.unwrap_or(0.0);
    let eff_yes = yes_price_low * (1.0 + spread_val + slip_val);
    let eff_no = no_price * (1.0 + spread_val + slip_val);
    let raw_cost = eff_yes + eff_no;
    let total_cost = raw_cost * (1.0 + fee_val);
    let exit_fees = 2.0 * fee_val;
    let net_profit = 2.0 - exit_fees - total_cost;

    println!("\n{}", "=".repeat(60));
    println!("  BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    if spread_val > 0.0 || slip_val > 0.0 || fee_val > 0.0 {
        println!("  -- Trading Costs --");
        println!("  Spread/leg      : {:.1}%", spread_val * 100.0);
        println!("  Slippage        : {:.1}%", slip_val * 100.0);
        println!("  Platform fee    : {:.1}%", fee_val * 100.0);
        println!("  Nominal prices  : YES_low={:.2} NO_high={:.2}", yes_price_low, no_price);
        println!("  Effective prices: YES_low={:.4} NO_high={:.4}", eff_yes, eff_no);
        println!("  Effective cost  : {:.4} (nominal: {:.4})", total_cost, yes_price_low + no_price);
        println!("  Net profit/win  : {:.4} (nominal: {:.4})", net_profit, 2.0 - yes_price_low - no_price);
        println!("  Effective ROI   : {:.1}%", (net_profit / total_cost) * 100.0);
        println!("  ---");
    }
    println!("  Total trades    : {}", summary.total_trades);
    println!(
        "  Winning trades  : {} ({:.1}%)",
        summary.winning_trades, summary.win_rate
    );
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
        println!(
            "  {:<12} {:<12} {:<22} {:<10} {:<10} {:<8}",
            "Entry", "Expiry", "Range", "BTC Exp.", "PnL", "Result"
        );
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
                if t.won { "WIN" } else { "LOSS" },
            );
        }
    }

    // Advanced analytics
    let report = analytics::full_report(&summary, &candles, duration_days as u64);

    println!("\n{}", "=".repeat(60));
    println!("  ADVANCED ANALYTICS");
    println!("{}", "=".repeat(60));

    println!("\n  Kelly Criterion (position sizing):");
    println!(
        "    Full Kelly    : {:.1}% of bankroll",
        report.kelly.full_kelly * 100.0
    );
    println!(
        "    Half Kelly    : {:.1}% (recommended)",
        report.kelly.half_kelly * 100.0
    );
    println!(
        "    Quarter Kelly : {:.1}% (conservative)",
        report.kelly.quarter_kelly * 100.0
    );
    println!("    Edge          : {:.4}", report.kelly.edge);
    println!("    Win/Loss ratio: {:.2}", report.kelly.win_loss_ratio);

    println!("\n  Risk-Adjusted Returns:");
    println!("    Sharpe ratio  : {:.2}", report.risk.sharpe_ratio);
    println!("    Sortino ratio : {:.2}", report.risk.sortino_ratio);
    println!(
        "    Max drawdown  : {:.1}% ({:.2} abs)",
        report.risk.max_drawdown_pct, report.risk.max_drawdown_abs
    );
    println!("    Calmar ratio  : {:.2}", report.risk.calmar_ratio);
    println!("    Profit factor : {:.2}", report.risk.profit_factor);

    println!("\n  Volatility Analysis:");
    println!("    Daily vol     : {:.2}%", report.volatility.daily_vol);
    println!(
        "    Annual vol    : {:.1}%",
        report.volatility.annualized_vol
    );
    println!("    ATR(14)       : {:.2}%", report.volatility.atr_14_pct);
    println!(
        "    Suggested rng : +/-{:.1}%",
        report.volatility.suggested_range_pct
    );
    println!(
        "    Vol regime    : {:.2}x (>1 = elevated)",
        report.volatility.vol_regime
    );

    println!("\n  Monte Carlo (10k simulations):");
    println!("    Median PnL    : {:.2}", report.monte_carlo.median_pnl);
    println!(
        "    5th–95th pct  : [{:.2}, {:.2}]",
        report.monte_carlo.pnl_5th, report.monte_carlo.pnl_95th
    );
    println!("    P(profit)     : {:.1}%", report.monte_carlo.prob_profit);
    println!(
        "    Max DD (95th) : {:.2}",
        report.monte_carlo.max_drawdown_95th
    );

    println!("\n  Expected Value:");
    println!(
        "    EV per trade  : {:.4} ({:.2}%)",
        report.expected_value.ev_per_trade, report.expected_value.ev_pct
    );
    println!(
        "    Breakeven WR  : {:.1}%",
        report.expected_value.breakeven_win_rate
    );
    println!(
        "    Actual WR     : {:.1}%",
        report.expected_value.actual_win_rate
    );
    println!(
        "    Edge over BE  : {:+.1}pp",
        report.expected_value.edge_over_breakeven
    );
    println!("{}", "=".repeat(60));

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
