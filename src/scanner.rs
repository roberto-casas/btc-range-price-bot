use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Datelike, Duration, Utc};
use regex::Regex;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::types::{Candle, Leg, Market, Pair, Structure};

// ── API Endpoints ──────────────────────────────────────────────────────────────
const COINGECKO_URL: &str = "https://api.coingecko.com/api/v3/simple/price";
const GAMMA_EVENTS_URL: &str = "https://gamma-api.polymarket.com/events";
const CLOB_PRICE_URL: &str = "https://clob.polymarket.com/price";
const COINGECKO_HISTORY_URL: &str =
    "https://api.coingecko.com/api/v3/coins/bitcoin/market_chart";

// ── Strategy configuration ─────────────────────────────────────────────────────
const LOW_RANGE: (f64, f64) = (0.80, 0.97);
const HIGH_RANGE: (f64, f64) = (1.03, 1.25);
const REQUEST_TIMEOUT_SECS: u64 = 15;

// ── Internal deserialization helpers ──────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct CoinGeckoResp {
    bitcoin: CoinGeckoBtc,
}

#[derive(Deserialize, Debug)]
struct CoinGeckoBtc {
    usd: f64,
}

#[derive(Deserialize, Debug, Default)]
struct GammaEvent {
    #[serde(default)]
    markets: Vec<GammaMarket>,
    #[serde(rename = "endDate", default)]
    end_date: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GammaMarket {
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(rename = "endDate", default)]
    end_date: Option<String>,
    #[serde(rename = "endDateIso", default)]
    end_date_iso: Option<String>,
    #[serde(rename = "outcomePrices", default)]
    outcome_prices: serde_json::Value,
    #[serde(rename = "clobTokenIds", default)]
    clob_token_ids: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct ClobPriceResp {
    price: String,
}

// ── BTC price ─────────────────────────────────────────────────────────────────

/// Fetch current BTC/USD price from CoinGecko.
pub async fn get_btc_price(client: &reqwest::Client) -> Result<f64> {
    let resp: CoinGeckoResp = client
        .get(COINGECKO_URL)
        .query(&[("ids", "bitcoin"), ("vs_currencies", "usd")])
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send CoinGecko request")?
        .json()
        .await
        .context("Failed to parse CoinGecko response")?;
    Ok(resp.bitcoin.usd)
}

// ── Slug generation ───────────────────────────────────────────────────────────

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "january",
        2 => "february",
        3 => "march",
        4 => "april",
        5 => "may",
        6 => "june",
        7 => "july",
        8 => "august",
        9 => "september",
        10 => "october",
        11 => "november",
        _ => "december",
    }
}

/// Generate "bitcoin-above-on-{month}-{day}" slugs for the next `days_ahead` days.
fn generate_btc_event_slugs(days_ahead: u32) -> Vec<String> {
    let now = Utc::now();
    (0..days_ahead)
        .map(|i| {
            let date = now + Duration::days(i as i64);
            let day = date.format("%-d").to_string();
            let month = month_name(date.month());
            format!("bitcoin-above-on-{month}-{day}")
        })
        .collect()
}

fn event_slugs_for_timeframe(timeframe: &str) -> Vec<String> {
    let days = if timeframe == "week" { 7 } else { 30 };
    generate_btc_event_slugs(days)
}

// ── Gamma API helpers ─────────────────────────────────────────────────────────

async fn get_event_by_slug(client: &reqwest::Client, slug: &str) -> Option<GammaEvent> {
    let resp = client
        .get(GAMMA_EVENTS_URL)
        .query(&[("slug", slug)])
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .ok()?;

    let mut events: Vec<GammaEvent> = resp.json().await.ok()?;
    if events.is_empty() {
        None
    } else {
        Some(events.remove(0))
    }
}

fn parse_strings_from_value(val: &serde_json::Value) -> Vec<String> {
    match val {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        serde_json::Value::String(s) => {
            serde_json::from_str::<Vec<String>>(s).unwrap_or_default()
        }
        _ => vec![],
    }
}

fn parse_threshold(question: &str) -> Option<f64> {
    let patterns = [
        r"\$([0-9,]+(?:\.[0-9]+)?)",
        r"([0-9,]+(?:\.[0-9]+)?)\s*(?:USD|dollars?)",
    ];
    for pat in &patterns {
        if let Ok(re) = Regex::new(pat) {
            if let Some(cap) = re.captures(question) {
                if let Ok(val) = cap[1].replace(',', "").parse::<f64>() {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn parse_end_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim_end_matches('Z');
    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(&format!("{s}+00:00")) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try NaiveDateTime fallback formats
    for fmt in &[
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d.and_hms_opt(0, 0, 0).unwrap().and_utc());
        }
    }
    None
}

// ── CLOB price ────────────────────────────────────────────────────────────────

pub async fn get_token_price_clob(
    client: &reqwest::Client,
    token_id: &str,
    side: &str,
) -> Option<f64> {
    let resp = client
        .get(CLOB_PRICE_URL)
        .query(&[("token_id", token_id), ("side", side)])
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    let data: ClobPriceResp = resp.json().await.ok()?;
    data.price.parse::<f64>().ok()
}

// ── Candidate building ────────────────────────────────────────────────────────

async fn build_candidate_markets(
    client: &reqwest::Client,
    slugs: &[String],
    btc_price: f64,
) -> Vec<Market> {
    let (low_min, low_max) = LOW_RANGE;
    let (high_min, high_max) = HIGH_RANGE;
    let now = Utc::now();
    let mut candidates = Vec::new();

    for slug in slugs {
        let event = match get_event_by_slug(client, slug).await {
            Some(e) => e,
            None => {
                debug!("No event for slug: {slug}");
                continue;
            }
        };

        for market in &event.markets {
            let question = market
                .question
                .as_deref()
                .or(market.title.as_deref())
                .unwrap_or("")
                .to_string();

            let threshold = match parse_threshold(&question) {
                Some(t) => t,
                None => continue,
            };

            let date_str = market
                .end_date
                .as_deref()
                .or(market.end_date_iso.as_deref())
                .or(event.end_date.as_deref())
                .unwrap_or("");

            let end_date = match parse_end_date(date_str) {
                Some(d) => d,
                None => continue,
            };

            let days_until = (end_date - now).num_days();

            let prices = parse_strings_from_value(&market.outcome_prices);
            let yes_price = match prices.first().and_then(|s| s.parse::<f64>().ok()) {
                Some(p) => p,
                None => continue,
            };

            let ids = parse_strings_from_value(&market.clob_token_ids);
            let yes_token_id = match ids.first() {
                Some(id) if !id.is_empty() => id.clone(),
                _ => continue,
            };
            let no_token_id = ids.get(1).cloned();

            let ratio = threshold / btc_price;
            let leg = if (low_min..=low_max).contains(&ratio) {
                Leg::Low
            } else if (high_min..=high_max).contains(&ratio) {
                Leg::High
            } else {
                continue;
            };

            candidates.push(Market {
                question,
                threshold,
                ratio,
                end_date,
                days_until,
                yes_price,
                yes_token_id,
                no_token_id,
                leg,
                slug: slug.clone(),
            });
        }
    }

    candidates
}

// ── Structure calculation ─────────────────────────────────────────────────────

/// Calculate delta-neutral range structure metrics.
pub fn calculate_structure(yes_price_low: f64, yes_price_high: f64, balance: f64) -> Structure {
    let no_price = 1.0 - yes_price_high;
    let cost_per_unit = yes_price_low + no_price;
    let profit_in_rng = 2.0 - cost_per_unit;
    let rr_reward = profit_in_rng / cost_per_unit;
    let profit_pct = rr_reward * 100.0;

    let per_leg = balance / 2.0;
    let units = f64::min(per_leg / yes_price_low, per_leg / no_price);
    let cost_low = units * yes_price_low;
    let cost_high = units * no_price;
    let total_cost = cost_low + cost_high;
    let expected_profit = units * profit_in_rng;
    let max_drawdown = total_cost;

    Structure {
        yes_price_low,
        yes_price_high,
        no_price,
        cost_per_unit,
        profit_in_rng,
        rr_reward,
        profit_pct,
        balance,
        per_leg,
        units,
        cost_low,
        cost_high,
        total_cost,
        expected_profit,
        max_drawdown,
    }
}

// ── Main scanning ─────────────────────────────────────────────────────────────

/// Find and sort delta-neutral pairs by profit.
pub async fn find_best_pairs(
    client: &reqwest::Client,
    btc_price: f64,
    timeframe: &str,
) -> Result<Vec<Pair>> {
    let slugs = event_slugs_for_timeframe(timeframe);
    if slugs.is_empty() {
        return Err(anyhow!("No slugs generated"));
    }

    let candidates = build_candidate_markets(client, &slugs, btc_price).await;

    let low_mkts: Vec<_> = candidates.iter().filter(|m| m.leg == Leg::Low).collect();
    let high_mkts: Vec<_> = candidates.iter().filter(|m| m.leg == Leg::High).collect();

    if low_mkts.is_empty() || high_mkts.is_empty() {
        return Err(anyhow!(
            "Insufficient markets: {} low, {} high",
            low_mkts.len(),
            high_mkts.len()
        ));
    }

    // Fetch live CLOB prices concurrently
    let mut low_prices: std::collections::HashMap<String, f64> = Default::default();
    for m in &low_mkts {
        let p = if !m.yes_token_id.is_empty() {
            get_token_price_clob(client, &m.yes_token_id, "buy")
                .await
                .filter(|&p| (0.01..0.99).contains(&p))
                .unwrap_or(m.yes_price)
        } else {
            m.yes_price
        };
        low_prices.insert(m.yes_token_id.clone(), p);
    }

    let mut high_prices: std::collections::HashMap<String, f64> = Default::default();
    for m in &high_mkts {
        let p = if !m.yes_token_id.is_empty() {
            get_token_price_clob(client, &m.yes_token_id, "buy")
                .await
                .filter(|&p| (0.01..0.99).contains(&p))
                .unwrap_or(m.yes_price)
        } else {
            m.yes_price
        };
        high_prices.insert(m.yes_token_id.clone(), p);
    }

    let mut pairs: Vec<Pair> = Vec::new();

    for low in &low_mkts {
        let p_low = *low_prices.get(&low.yes_token_id).unwrap_or(&low.yes_price);

        for high in &high_mkts {
            let expiry_diff = (low.end_date - high.end_date).num_days().abs();
            if expiry_diff > 0 {
                continue;
            }

            let p_high = *high_prices.get(&high.yes_token_id).unwrap_or(&high.yes_price);
            let calc = calculate_structure(p_low, p_high, 100.0);

            pairs.push(Pair {
                low_market: (*low).clone(),
                high_market: (*high).clone(),
                yes_price_low: p_low,
                yes_price_high: p_high,
                profit_in_rng: calc.profit_in_rng,
                profit_pct: calc.profit_pct,
                rr_reward: calc.rr_reward,
            });
        }
    }

    pairs.sort_by(|a, b| b.profit_in_rng.partial_cmp(&a.profit_in_rng).unwrap());
    Ok(pairs)
}

// ── Historical price data for backtesting ─────────────────────────────────────

/// Fetch daily BTC/USD candles from CoinGecko (max 365 days).
pub async fn fetch_historical_btc(client: &reqwest::Client, days: u32) -> Result<Vec<Candle>> {
    #[derive(Deserialize)]
    struct ChartResp {
        prices: Vec<[f64; 2]>,
    }

    let days_str = days.to_string();
    let resp: ChartResp = client
        .get(COINGECKO_HISTORY_URL)
        .query(&[
            ("vs_currency", "usd"),
            ("days", &days_str),
            ("interval", "daily"),
        ])
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to fetch historical BTC")?
        .json()
        .await
        .context("Failed to parse historical BTC response")?;

    let candles: Vec<Candle> = resp
        .prices
        .iter()
        .map(|[ts_ms, price]| {
            use chrono::TimeZone;
            let ts = Utc.timestamp_millis_opt(*ts_ms as i64).unwrap();
            Candle {
                timestamp: ts,
                open: *price,
                high: *price,
                low: *price,
                close: *price,
            }
        })
        .collect();

    if candles.is_empty() {
        warn!("No historical candles received");
    }
    Ok(candles)
}
