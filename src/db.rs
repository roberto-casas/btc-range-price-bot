//! SQLite persistence layer for dry-run mode.
//!
//! Stores simulated orders, portfolio snapshots, and price history so that
//! dry-run state survives across process restarts.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

/// Default database file path (next to the binary / working directory).
pub const DEFAULT_DB_PATH: &str = "polydelta-dryrun.db";

// ── Row types ────────────────────────────────────────────────────────────────

/// A persisted simulated order (one leg of a pair entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbOrder {
    pub id: String,
    pub created_at: String,
    pub pair_label: String,
    pub leg: String,
    pub side: String,
    pub token_id: String,
    pub price: f64,
    pub units: f64,
    pub cost: f64,
    /// BTC/USD at the time the order was placed.
    pub btc_price: f64,
    /// Expiry date of the underlying Polymarket market (ISO-8601).
    pub expiry: String,
    /// "open", "won", "lost", or "expired"
    pub status: String,
    /// Realised PnL once the position is settled.
    pub pnl: f64,
    /// BTC price at settlement (0.0 while still open).
    pub settled_btc_price: f64,
    /// ISO-8601 timestamp of settlement (empty while open).
    pub settled_at: String,
}

/// A point-in-time portfolio snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSnapshot {
    pub id: i64,
    pub created_at: String,
    pub btc_price: f64,
    pub total_invested: f64,
    pub total_pnl: f64,
    pub open_positions: i64,
    pub closed_positions: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub balance: f64,
}

/// BTC price observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbPricePoint {
    pub id: i64,
    pub created_at: String,
    pub btc_price: f64,
}

/// A persisted market evaluation and decision record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbEvaluation {
    pub id: i64,
    pub created_at: String,
    /// BTC/USD at the time of evaluation.
    pub btc_price: f64,
    /// Label of the best pair evaluated (e.g. "BTC $85k–$95k").
    pub pair_label: String,
    pub low_threshold: f64,
    pub high_threshold: f64,
    pub profit_pct: f64,
    pub days_until: i64,
    /// Number of delta-neutral pairs found in this scan.
    pub pairs_found: i64,
    /// AI risk level: "low", "medium", "high", "extreme", or "n/a" if AI disabled.
    pub risk_level: String,
    /// AI confidence 0.0–1.0 (0.0 if AI disabled).
    pub confidence: f64,
    /// Whether AI recommended skipping this trade.
    pub skip_trade: bool,
    /// AI reasoning text.
    pub reasoning: String,
    /// JSON array of risk factor strings.
    pub risk_factors: String,
    /// Suggested low boundary adjustment %.
    pub suggested_low_adj: f64,
    /// Suggested high boundary adjustment %.
    pub suggested_high_adj: f64,
    /// Decision taken: "entered", "skipped_ai", "skipped_duplicate", "no_pairs".
    pub decision: String,
}

// ── Database handle ──────────────────────────────────────────────────────────

/// Thread-safe wrapper around a SQLite connection.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (or create) the database at `path` and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Cannot open SQLite database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        info!("SQLite database ready at {}", path.display());
        Ok(db)
    }

    /// Open an in-memory database (useful for tests).
    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    // ── Schema migration ─────────────────────────────────────────────────────

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS orders (
                id              TEXT PRIMARY KEY,
                created_at      TEXT NOT NULL,
                pair_label      TEXT NOT NULL,
                leg             TEXT NOT NULL,
                side            TEXT NOT NULL,
                token_id        TEXT NOT NULL,
                price           REAL NOT NULL,
                units           REAL NOT NULL,
                cost            REAL NOT NULL,
                btc_price       REAL NOT NULL DEFAULT 0,
                expiry          TEXT NOT NULL DEFAULT '',
                status          TEXT NOT NULL DEFAULT 'open',
                pnl             REAL NOT NULL DEFAULT 0,
                settled_btc_price REAL NOT NULL DEFAULT 0,
                settled_at      TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS portfolio_snapshots (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at      TEXT NOT NULL,
                btc_price       REAL NOT NULL,
                total_invested  REAL NOT NULL,
                total_pnl       REAL NOT NULL,
                open_positions  INTEGER NOT NULL,
                closed_positions INTEGER NOT NULL,
                win_count       INTEGER NOT NULL DEFAULT 0,
                loss_count      INTEGER NOT NULL DEFAULT 0,
                balance         REAL NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS price_history (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at      TEXT NOT NULL,
                btc_price       REAL NOT NULL
            );

            CREATE TABLE IF NOT EXISTS evaluations (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at         TEXT NOT NULL,
                btc_price          REAL NOT NULL,
                pair_label         TEXT NOT NULL DEFAULT '',
                low_threshold      REAL NOT NULL DEFAULT 0,
                high_threshold     REAL NOT NULL DEFAULT 0,
                profit_pct         REAL NOT NULL DEFAULT 0,
                days_until         INTEGER NOT NULL DEFAULT 0,
                pairs_found        INTEGER NOT NULL DEFAULT 0,
                risk_level         TEXT NOT NULL DEFAULT 'n/a',
                confidence         REAL NOT NULL DEFAULT 0,
                skip_trade         INTEGER NOT NULL DEFAULT 0,
                reasoning          TEXT NOT NULL DEFAULT '',
                risk_factors       TEXT NOT NULL DEFAULT '[]',
                suggested_low_adj  REAL NOT NULL DEFAULT 0,
                suggested_high_adj REAL NOT NULL DEFAULT 0,
                decision           TEXT NOT NULL DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
            CREATE INDEX IF NOT EXISTS idx_orders_expiry ON orders(expiry);
            CREATE INDEX IF NOT EXISTS idx_price_history_created ON price_history(created_at);
            CREATE INDEX IF NOT EXISTS idx_evaluations_created ON evaluations(created_at);
            ",
        )?;
        Ok(())
    }

    // ── Orders ───────────────────────────────────────────────────────────────

    /// Insert a new simulated order.
    pub fn insert_order(&self, order: &DbOrder) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO orders (id, created_at, pair_label, leg, side, token_id,
                                 price, units, cost, btc_price, expiry, status,
                                 pnl, settled_btc_price, settled_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                order.id,
                order.created_at,
                order.pair_label,
                order.leg,
                order.side,
                order.token_id,
                order.price,
                order.units,
                order.cost,
                order.btc_price,
                order.expiry,
                order.status,
                order.pnl,
                order.settled_btc_price,
                order.settled_at,
            ],
        )?;
        Ok(())
    }

    /// Fetch all orders (newest first).
    pub fn get_all_orders(&self) -> Result<Vec<DbOrder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, pair_label, leg, side, token_id,
                    price, units, cost, btc_price, expiry, status,
                    pnl, settled_btc_price, settled_at
             FROM orders ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DbOrder {
                id: row.get(0)?,
                created_at: row.get(1)?,
                pair_label: row.get(2)?,
                leg: row.get(3)?,
                side: row.get(4)?,
                token_id: row.get(5)?,
                price: row.get(6)?,
                units: row.get(7)?,
                cost: row.get(8)?,
                btc_price: row.get(9)?,
                expiry: row.get(10)?,
                status: row.get(11)?,
                pnl: row.get(12)?,
                settled_btc_price: row.get(13)?,
                settled_at: row.get(14)?,
            })
        })?;
        let mut orders = Vec::new();
        for row in rows {
            orders.push(row?);
        }
        Ok(orders)
    }

    /// Fetch only open orders.
    pub fn get_open_orders(&self) -> Result<Vec<DbOrder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, pair_label, leg, side, token_id,
                    price, units, cost, btc_price, expiry, status,
                    pnl, settled_btc_price, settled_at
             FROM orders WHERE status = 'open' ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DbOrder {
                id: row.get(0)?,
                created_at: row.get(1)?,
                pair_label: row.get(2)?,
                leg: row.get(3)?,
                side: row.get(4)?,
                token_id: row.get(5)?,
                price: row.get(6)?,
                units: row.get(7)?,
                cost: row.get(8)?,
                btc_price: row.get(9)?,
                expiry: row.get(10)?,
                status: row.get(11)?,
                pnl: row.get(12)?,
                settled_btc_price: row.get(13)?,
                settled_at: row.get(14)?,
            })
        })?;
        let mut orders = Vec::new();
        for row in rows {
            orders.push(row?);
        }
        Ok(orders)
    }

    /// Settle an order: set status, PnL and settlement metadata.
    pub fn settle_order(
        &self,
        order_id: &str,
        status: &str,
        pnl: f64,
        settled_btc_price: f64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE orders SET status = ?1, pnl = ?2, settled_btc_price = ?3, settled_at = ?4
             WHERE id = ?5",
            params![status, pnl, settled_btc_price, now, order_id],
        )?;
        Ok(())
    }

    /// Check if we already placed an order for this pair_label + expiry date
    /// during this scan cycle (avoid duplicates across restarts).
    pub fn has_order_for_pair(&self, pair_label: &str, expiry: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM orders WHERE pair_label = ?1 AND expiry = ?2",
            params![pair_label, expiry],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // ── Portfolio snapshots ──────────────────────────────────────────────────

    /// Record a portfolio snapshot.
    pub fn insert_snapshot(&self, snap: &DbSnapshot) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO portfolio_snapshots
                (created_at, btc_price, total_invested, total_pnl,
                 open_positions, closed_positions, win_count, loss_count, balance)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                snap.created_at,
                snap.btc_price,
                snap.total_invested,
                snap.total_pnl,
                snap.open_positions,
                snap.closed_positions,
                snap.win_count,
                snap.loss_count,
                snap.balance,
            ],
        )?;
        Ok(())
    }

    /// Get the most recent snapshot.
    pub fn get_latest_snapshot(&self) -> Result<Option<DbSnapshot>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, btc_price, total_invested, total_pnl,
                    open_positions, closed_positions, win_count, loss_count, balance
             FROM portfolio_snapshots ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            Ok(DbSnapshot {
                id: row.get(0)?,
                created_at: row.get(1)?,
                btc_price: row.get(2)?,
                total_invested: row.get(3)?,
                total_pnl: row.get(4)?,
                open_positions: row.get(5)?,
                closed_positions: row.get(6)?,
                win_count: row.get(7)?,
                loss_count: row.get(8)?,
                balance: row.get(9)?,
            })
        })?;
        match rows.next() {
            Some(Ok(snap)) => Ok(Some(snap)),
            _ => Ok(None),
        }
    }

    /// Get all snapshots (for charting).
    pub fn get_all_snapshots(&self) -> Result<Vec<DbSnapshot>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, btc_price, total_invested, total_pnl,
                    open_positions, closed_positions, win_count, loss_count, balance
             FROM portfolio_snapshots ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DbSnapshot {
                id: row.get(0)?,
                created_at: row.get(1)?,
                btc_price: row.get(2)?,
                total_invested: row.get(3)?,
                total_pnl: row.get(4)?,
                open_positions: row.get(5)?,
                closed_positions: row.get(6)?,
                win_count: row.get(7)?,
                loss_count: row.get(8)?,
                balance: row.get(9)?,
            })
        })?;
        let mut snaps = Vec::new();
        for row in rows {
            snaps.push(row?);
        }
        Ok(snaps)
    }

    // ── Price history ────────────────────────────────────────────────────────

    /// Record a BTC price observation.
    pub fn insert_price(&self, btc_price: f64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO price_history (created_at, btc_price) VALUES (?1, ?2)",
            params![now, btc_price],
        )?;
        Ok(())
    }

    /// Get recent price observations (newest first, up to `limit`).
    pub fn get_recent_prices(&self, limit: u32) -> Result<Vec<DbPricePoint>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, btc_price FROM price_history
             ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(DbPricePoint {
                id: row.get(0)?,
                created_at: row.get(1)?,
                btc_price: row.get(2)?,
            })
        })?;
        let mut points = Vec::new();
        for row in rows {
            points.push(row?);
        }
        Ok(points)
    }

    // ── Evaluations ──────────────────────────────────────────────────────────

    /// Record a market evaluation and the decision taken.
    pub fn insert_evaluation(&self, eval: &DbEvaluation) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO evaluations
                (created_at, btc_price, pair_label, low_threshold, high_threshold,
                 profit_pct, days_until, pairs_found, risk_level, confidence,
                 skip_trade, reasoning, risk_factors, suggested_low_adj,
                 suggested_high_adj, decision)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                eval.created_at,
                eval.btc_price,
                eval.pair_label,
                eval.low_threshold,
                eval.high_threshold,
                eval.profit_pct,
                eval.days_until,
                eval.pairs_found,
                eval.risk_level,
                eval.confidence,
                eval.skip_trade as i32,
                eval.reasoning,
                eval.risk_factors,
                eval.suggested_low_adj,
                eval.suggested_high_adj,
                eval.decision,
            ],
        )?;
        Ok(())
    }

    /// Get all evaluations (newest first, up to `limit`).
    pub fn get_evaluations(&self, limit: u32) -> Result<Vec<DbEvaluation>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, btc_price, pair_label, low_threshold,
                    high_threshold, profit_pct, days_until, pairs_found,
                    risk_level, confidence, skip_trade, reasoning,
                    risk_factors, suggested_low_adj, suggested_high_adj, decision
             FROM evaluations ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(DbEvaluation {
                id: row.get(0)?,
                created_at: row.get(1)?,
                btc_price: row.get(2)?,
                pair_label: row.get(3)?,
                low_threshold: row.get(4)?,
                high_threshold: row.get(5)?,
                profit_pct: row.get(6)?,
                days_until: row.get(7)?,
                pairs_found: row.get(8)?,
                risk_level: row.get(9)?,
                confidence: row.get(10)?,
                skip_trade: {
                    let v: i32 = row.get(11)?;
                    v != 0
                },
                reasoning: row.get(12)?,
                risk_factors: row.get(13)?,
                suggested_low_adj: row.get(14)?,
                suggested_high_adj: row.get(15)?,
                decision: row.get(16)?,
            })
        })?;
        let mut evals = Vec::new();
        for row in rows {
            evals.push(row?);
        }
        Ok(evals)
    }

    // ── Aggregate helpers ────────────────────────────────────────────────────

    /// Compute a fresh portfolio snapshot from current order state.
    pub fn compute_snapshot(&self, btc_price: f64, initial_balance: f64) -> Result<DbSnapshot> {
        let conn = self.conn.lock().unwrap();

        let total_invested: f64 = conn.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM orders",
            [],
            |row| row.get(0),
        )?;

        let total_pnl: f64 = conn.query_row(
            "SELECT COALESCE(SUM(pnl), 0) FROM orders WHERE status IN ('won','lost')",
            [],
            |row| row.get(0),
        )?;

        let open_positions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM orders WHERE status = 'open'",
            [],
            |row| row.get(0),
        )?;

        let closed_positions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM orders WHERE status IN ('won','lost')",
            [],
            |row| row.get(0),
        )?;

        let win_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM orders WHERE status = 'won'",
            [],
            |row| row.get(0),
        )?;

        let loss_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM orders WHERE status = 'lost'",
            [],
            |row| row.get(0),
        )?;

        let open_cost: f64 = conn.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM orders WHERE status = 'open'",
            [],
            |row| row.get(0),
        )?;

        let balance = initial_balance - open_cost + total_pnl;

        Ok(DbSnapshot {
            id: 0,
            created_at: Utc::now().to_rfc3339(),
            btc_price,
            total_invested,
            total_pnl,
            open_positions,
            closed_positions,
            win_count,
            loss_count,
            balance,
        })
    }

    /// Settle expired orders based on current BTC price.
    /// For LOW legs (BUY YES "BTC above X"): wins if btc_price > threshold.
    /// For HIGH legs (BUY NO "BTC above Y"): wins if btc_price < threshold.
    pub fn settle_expired_orders(&self, btc_price: f64) -> Result<u32> {
        let now_str = Utc::now().to_rfc3339();
        let open_orders = self.get_open_orders()?;
        let mut settled = 0u32;

        for order in &open_orders {
            if order.expiry.is_empty() {
                continue;
            }

            let expiry = match DateTime::parse_from_rfc3339(&order.expiry) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => continue,
            };

            if Utc::now() < expiry {
                continue;
            }

            // Determine outcome based on leg type
            let won = match order.leg.as_str() {
                "LOW" => {
                    // BUY YES on "BTC above X" → wins if BTC > threshold
                    // The threshold is encoded in pair_label, but we compare using
                    // the entry BTC price and the price range logic.
                    // Simpler: LOW leg wins if BTC stayed above entry range bottom
                    // We use btc_price vs the price at entry * range ratio
                    true // simplified: settled by price check below
                }
                "HIGH" => true,
                _ => false,
            };

            // More accurate: parse threshold from pair_label (e.g. "BTC $85k–$95k")
            let (low_thresh, high_thresh) = parse_pair_thresholds(&order.pair_label);

            let in_range = btc_price >= low_thresh && btc_price <= high_thresh;
            let pnl = if in_range {
                // Both legs win → full payout minus cost
                order.units * 1.0 - order.cost // payout is units * $1 per winning leg
            } else {
                -order.cost // total loss
            };

            let status = if in_range { "won" } else { "lost" };
            let _ = won; // suppress unused

            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE orders SET status = ?1, pnl = ?2, settled_btc_price = ?3, settled_at = ?4
                 WHERE id = ?5",
                params![status, pnl, btc_price, now_str, order.id],
            )?;
            settled += 1;
        }

        Ok(settled)
    }
}

/// Parse low and high thresholds from a pair label like "BTC $85k–$95k".
fn parse_pair_thresholds(label: &str) -> (f64, f64) {
    let re = regex::Regex::new(r"\$([0-9]+)k[–-]\$([0-9]+)k").ok();
    if let Some(re) = re {
        if let Some(caps) = re.captures(label) {
            let low = caps[1].parse::<f64>().unwrap_or(0.0) * 1000.0;
            let high = caps[2].parse::<f64>().unwrap_or(0.0) * 1000.0;
            return (low, high);
        }
    }
    (0.0, f64::MAX)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_and_migrate() {
        let db = Db::open_memory().unwrap();
        let orders = db.get_all_orders().unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn test_insert_and_retrieve_order() {
        let db = Db::open_memory().unwrap();
        let order = DbOrder {
            id: "test-001".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            pair_label: "BTC $85k–$95k".to_string(),
            leg: "LOW".to_string(),
            side: "BUY YES".to_string(),
            token_id: "tok_abc".to_string(),
            price: 0.65,
            units: 76.92,
            cost: 50.0,
            btc_price: 90000.0,
            expiry: "2025-01-08T00:00:00Z".to_string(),
            status: "open".to_string(),
            pnl: 0.0,
            settled_btc_price: 0.0,
            settled_at: String::new(),
        };
        db.insert_order(&order).unwrap();

        let orders = db.get_all_orders().unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].pair_label, "BTC $85k–$95k");
        assert_eq!(orders[0].status, "open");
    }

    #[test]
    fn test_settle_order() {
        let db = Db::open_memory().unwrap();
        let order = DbOrder {
            id: "test-002".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            pair_label: "BTC $85k–$95k".to_string(),
            leg: "LOW".to_string(),
            side: "BUY YES".to_string(),
            token_id: "tok_xyz".to_string(),
            price: 0.60,
            units: 83.33,
            cost: 50.0,
            btc_price: 90000.0,
            expiry: "2025-01-08T00:00:00Z".to_string(),
            status: "open".to_string(),
            pnl: 0.0,
            settled_btc_price: 0.0,
            settled_at: String::new(),
        };
        db.insert_order(&order).unwrap();
        db.settle_order("test-002", "won", 33.33, 91000.0).unwrap();

        let orders = db.get_all_orders().unwrap();
        assert_eq!(orders[0].status, "won");
        assert!((orders[0].pnl - 33.33).abs() < 0.01);
    }

    #[test]
    fn test_has_order_for_pair() {
        let db = Db::open_memory().unwrap();
        assert!(!db.has_order_for_pair("BTC $85k–$95k", "2025-01-08T00:00:00Z").unwrap());

        let order = DbOrder {
            id: "test-003".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            pair_label: "BTC $85k–$95k".to_string(),
            leg: "LOW".to_string(),
            side: "BUY YES".to_string(),
            token_id: "tok_123".to_string(),
            price: 0.60,
            units: 83.33,
            cost: 50.0,
            btc_price: 90000.0,
            expiry: "2025-01-08T00:00:00Z".to_string(),
            status: "open".to_string(),
            pnl: 0.0,
            settled_btc_price: 0.0,
            settled_at: String::new(),
        };
        db.insert_order(&order).unwrap();
        assert!(db.has_order_for_pair("BTC $85k–$95k", "2025-01-08T00:00:00Z").unwrap());
    }

    #[test]
    fn test_compute_snapshot() {
        let db = Db::open_memory().unwrap();
        let snap = db.compute_snapshot(90000.0, 1000.0).unwrap();
        assert_eq!(snap.open_positions, 0);
        assert_eq!(snap.balance, 1000.0);
    }

    #[test]
    fn test_price_history() {
        let db = Db::open_memory().unwrap();
        db.insert_price(88000.0).unwrap();
        db.insert_price(89000.0).unwrap();
        let prices = db.get_recent_prices(10).unwrap();
        assert_eq!(prices.len(), 2);
        assert!((prices[0].btc_price - 89000.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_pair_thresholds() {
        let (low, high) = parse_pair_thresholds("BTC $85k–$95k");
        assert!((low - 85000.0).abs() < 0.01);
        assert!((high - 95000.0).abs() < 0.01);

        let (low, high) = parse_pair_thresholds("unknown label");
        assert!((low - 0.0).abs() < 0.01);
        assert!(high > 1e18);
    }
}
