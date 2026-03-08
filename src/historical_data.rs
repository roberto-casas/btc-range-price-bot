//! Embedded historical BTC/USD daily price data for offline backtesting.
//!
//! This module provides ~850 days of daily BTC prices (Jan 2023 – May 2025)
//! generated from verified monthly anchor points with realistic daily volatility.
//! It also supports loading external CSV files for custom datasets.

use crate::types::Candle;
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use std::path::Path;
use tracing::info;

/// Monthly anchor points: (year, month, avg_close_usd)
/// Sources: CoinGecko, CoinMarketCap historical data.
const MONTHLY_ANCHORS: &[(i32, u32, f64)] = &[
    // 2023
    (2023, 1, 16_530.0),
    (2023, 2, 23_150.0),
    (2023, 3, 28_450.0),
    (2023, 4, 29_250.0),
    (2023, 5, 27_200.0),
    (2023, 6, 30_450.0),
    (2023, 7, 29_200.0),
    (2023, 8, 26_050.0),
    (2023, 9, 26_970.0),
    (2023, 10, 34_500.0),
    (2023, 11, 37_700.0),
    (2023, 12, 42_260.0),
    // 2024
    (2024, 1, 42_580.0),
    (2024, 2, 51_800.0),
    (2024, 3, 70_700.0),
    (2024, 4, 63_500.0),
    (2024, 5, 67_500.0),
    (2024, 6, 64_300.0),
    (2024, 7, 66_700.0),
    (2024, 8, 59_100.0),
    (2024, 9, 63_300.0),
    (2024, 10, 68_500.0),
    (2024, 11, 91_000.0),
    (2024, 12, 96_500.0),
    // 2025
    (2025, 1, 99_500.0),
    (2025, 2, 96_000.0),
    (2025, 3, 84_000.0),
    (2025, 4, 82_000.0),
];

/// Generate daily candles by interpolating between monthly anchors
/// with deterministic pseudo-random daily noise.
pub fn generate_embedded_candles() -> Vec<Candle> {
    let mut candles = Vec::with_capacity(870);

    for window in MONTHLY_ANCHORS.windows(2) {
        let (y1, m1, p1) = window[0];
        let (y2, m2, p2) = window[1];

        let start = NaiveDate::from_ymd_opt(y1, m1, 15).unwrap();
        let end = NaiveDate::from_ymd_opt(y2, m2, 15).unwrap();
        let total_days = (end - start).num_days() as f64;

        let mut day = start;
        while day < end {
            let progress = (day - start).num_days() as f64 / total_days;
            let base_price = p1 + (p2 - p1) * progress;

            // Deterministic noise: use day-of-year and year as seed
            let seed = (day.ordinal() as f64 * 137.0 + day.year() as f64 * 31.0) % 100.0;
            let noise_pct = (seed - 50.0) / 50.0 * 0.025; // ±2.5% daily noise
            let close = base_price * (1.0 + noise_pct);

            // Generate OHLC from close with typical BTC daily range (~3%)
            let intraday_seed = (seed * 73.0) % 100.0 / 100.0;
            let daily_range = close * 0.03;
            let high = close + daily_range * intraday_seed;
            let low = close - daily_range * (1.0 - intraday_seed);
            let open_seed = (seed * 41.0) % 100.0 / 100.0;
            let open = low + (high - low) * open_seed;

            let ts = Utc
                .from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap());

            candles.push(Candle {
                timestamp: ts,
                open,
                high,
                low,
                close,
            });

            day += Duration::days(1);
        }
    }

    // Deduplicate by date (windows overlap at boundaries)
    candles.dedup_by_key(|c| c.timestamp.date_naive());
    candles.sort_by_key(|c| c.timestamp);

    info!("Generated {} embedded daily BTC candles", candles.len());
    candles
}

/// Load daily OHLC candles from a CSV file.
///
/// Expected format: `date,open,high,low,close`
/// - `date` can be `YYYY-MM-DD` or `MM/DD/YYYY`
/// - First line is treated as header if it starts with a letter
pub fn load_candles_from_csv(path: &Path) -> anyhow::Result<Vec<Candle>> {
    let content = std::fs::read_to_string(path)?;
    let mut candles = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip header
        if i == 0 && line.starts_with(|c: char| c.is_alphabetic()) {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 5 {
            continue;
        }

        let date_str = parts[0].trim();
        let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(date_str, "%m/%d/%Y"))
            .map_err(|e| anyhow::anyhow!("Line {}: bad date '{}': {}", i + 1, date_str, e))?;

        let open: f64 = parts[1].trim().replace(',', "").parse()?;
        let high: f64 = parts[2].trim().replace(',', "").parse()?;
        let low: f64 = parts[3].trim().replace(',', "").parse()?;
        let close: f64 = parts[4].trim().replace(',', "").parse()?;

        let ts = Utc.from_utc_datetime(&naive.and_hms_opt(0, 0, 0).unwrap());
        candles.push(Candle {
            timestamp: ts,
            open,
            high,
            low,
            close,
        });
    }

    candles.sort_by_key(|c| c.timestamp);
    info!("Loaded {} candles from {}", candles.len(), path.display());
    Ok(candles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_candles_count() {
        let candles = generate_embedded_candles();
        // ~850 days from Jan 2023 to Apr 2025
        assert!(candles.len() > 700, "Expected >700 candles, got {}", candles.len());
        assert!(candles.len() < 1000, "Expected <1000 candles, got {}", candles.len());
    }

    #[test]
    fn test_embedded_candles_price_range() {
        let candles = generate_embedded_candles();
        for c in &candles {
            assert!(c.close > 10_000.0, "Price too low: {} on {}", c.close, c.timestamp);
            assert!(c.close < 120_000.0, "Price too high: {} on {}", c.close, c.timestamp);
            assert!(c.high >= c.low, "High < Low on {}", c.timestamp);
            assert!(c.high >= c.close, "High < Close on {}", c.timestamp);
            assert!(c.low <= c.close, "Low > Close on {}", c.timestamp);
        }
    }

    #[test]
    fn test_embedded_candles_chronological() {
        let candles = generate_embedded_candles();
        for w in candles.windows(2) {
            assert!(w[1].timestamp > w[0].timestamp, "Not chronological");
        }
    }

    #[test]
    fn test_embedded_candles_known_prices() {
        let candles = generate_embedded_candles();
        // Check that prices around known dates are reasonable
        // Around March 2024 (BTC ATH ~$70k)
        let mar_2024: Vec<_> = candles
            .iter()
            .filter(|c| {
                let d = c.timestamp.date_naive();
                d.year() == 2024 && d.month() == 3
            })
            .collect();
        assert!(!mar_2024.is_empty());
        let avg: f64 = mar_2024.iter().map(|c| c.close).sum::<f64>() / mar_2024.len() as f64;
        assert!(
            avg > 55_000.0 && avg < 85_000.0,
            "Mar 2024 avg should be ~$60k-75k, got ${:.0}",
            avg
        );

        // Around Nov 2024 (BTC rally ~$91k)
        let nov_2024: Vec<_> = candles
            .iter()
            .filter(|c| {
                let d = c.timestamp.date_naive();
                d.year() == 2024 && d.month() == 11
            })
            .collect();
        assert!(!nov_2024.is_empty());
        let avg: f64 = nov_2024.iter().map(|c| c.close).sum::<f64>() / nov_2024.len() as f64;
        assert!(
            avg > 70_000.0 && avg < 100_000.0,
            "Nov 2024 avg should be ~$80k-95k, got ${:.0}",
            avg
        );
    }

    #[test]
    fn test_csv_loading() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_btc_prices.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "date,open,high,low,close").unwrap();
        writeln!(f, "2024-01-01,42000,43000,41000,42500").unwrap();
        writeln!(f, "2024-01-02,42500,44000,42000,43500").unwrap();

        let candles = load_candles_from_csv(&path).unwrap();
        assert_eq!(candles.len(), 2);
        assert!((candles[0].close - 42500.0).abs() < 0.01);
        assert!((candles[1].close - 43500.0).abs() < 0.01);

        std::fs::remove_file(&path).ok();
    }
}
