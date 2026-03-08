use crate::db::{Db, DbOrder};
use crate::types::SimulatedOrder;
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

/// In dry-run mode: log and return a `SimulatedOrder` without calling any
/// real exchange API.
pub fn dry_run_order(
    pair_label: &str,
    leg: &str,
    side: &str,
    token_id: &str,
    price: f64,
    units: f64,
) -> SimulatedOrder {
    let cost = price * units;
    let order = SimulatedOrder {
        pair_label: pair_label.to_string(),
        leg: leg.to_string(),
        side: side.to_string(),
        token_id: token_id.to_string(),
        price,
        units,
        cost,
        dry_run: true,
    };

    info!(
        "[DRY-RUN] {} | {} {} @ {:.4} | units={:.4} cost={:.4} USDC",
        pair_label, leg, side, price, units, cost
    );

    order
}

/// Simulate placing both legs of a delta-neutral pair (dry-run only).
pub fn simulate_pair_entry(
    pair_label: &str,
    yes_token_id: &str,
    no_token_id: &str,
    yes_price_low: f64,
    no_price: f64,
    balance: f64,
) -> Vec<SimulatedOrder> {
    let per_leg = balance / 2.0;
    let units = f64::min(per_leg / yes_price_low, per_leg / no_price);

    vec![
        dry_run_order(pair_label, "LOW", "BUY YES", yes_token_id, yes_price_low, units),
        dry_run_order(pair_label, "HIGH", "BUY NO", no_token_id, no_price, units),
    ]
}

/// Simulate placing both legs **and persist** to the SQLite database.
/// Returns the simulated orders. Skips if the pair+expiry already exists in DB.
pub fn simulate_pair_entry_persistent(
    db: &Db,
    pair_label: &str,
    yes_token_id: &str,
    no_token_id: &str,
    yes_price_low: f64,
    no_price: f64,
    balance: f64,
    btc_price: f64,
    expiry: &str,
) -> Vec<SimulatedOrder> {
    // Dedup: don't re-enter the same pair+expiry
    if let Ok(true) = db.has_order_for_pair(pair_label, expiry) {
        info!(
            "[DRY-RUN] Skipping duplicate pair: {} (expiry {})",
            pair_label, expiry
        );
        return vec![];
    }

    let orders = simulate_pair_entry(
        pair_label,
        yes_token_id,
        no_token_id,
        yes_price_low,
        no_price,
        balance,
    );

    for sim in &orders {
        let db_order = DbOrder {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            pair_label: sim.pair_label.clone(),
            leg: sim.leg.clone(),
            side: sim.side.clone(),
            token_id: sim.token_id.clone(),
            price: sim.price,
            units: sim.units,
            cost: sim.cost,
            btc_price,
            expiry: expiry.to_string(),
            status: "open".to_string(),
            pnl: 0.0,
            settled_btc_price: 0.0,
            settled_at: String::new(),
        };
        if let Err(e) = db.insert_order(&db_order) {
            tracing::error!("[DRY-RUN] Failed to persist order: {e}");
        }
    }

    orders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_run_order_fields() {
        let order = dry_run_order("BTC 64k-70k", "LOW", "BUY YES", "tok123", 0.65, 10.0);
        assert!(order.dry_run);
        assert_eq!(order.leg, "LOW");
        assert_eq!(order.side, "BUY YES");
        assert!((order.cost - 6.5).abs() < 1e-9);
    }

    #[test]
    fn test_simulate_pair_entry_returns_two_legs() {
        let orders = simulate_pair_entry("BTC 64k-70k", "tok_yes", "tok_no", 0.65, 0.35, 100.0);
        assert_eq!(orders.len(), 2);
        assert!(orders.iter().all(|o| o.dry_run));
    }

    #[test]
    fn test_simulate_pair_entry_cost_allocation() {
        let orders = simulate_pair_entry("test", "y", "n", 0.6, 0.3, 100.0);
        let total_cost: f64 = orders.iter().map(|o| o.cost).sum();
        // total cost should be <= 100 USDC
        assert!(total_cost <= 100.0 + 1e-9);
    }

    #[test]
    fn test_persistent_entry_dedup() {
        let db = Db::open_memory().unwrap();
        let orders1 = simulate_pair_entry_persistent(
            &db, "BTC $85k–$95k", "tok_y", "tok_n",
            0.60, 0.35, 100.0, 90000.0, "2025-01-08T00:00:00Z",
        );
        assert_eq!(orders1.len(), 2);

        // Second call with same pair+expiry should be skipped
        let orders2 = simulate_pair_entry_persistent(
            &db, "BTC $85k–$95k", "tok_y", "tok_n",
            0.60, 0.35, 100.0, 90000.0, "2025-01-08T00:00:00Z",
        );
        assert_eq!(orders2.len(), 0);

        // All 2 orders should be in DB
        let all = db.get_all_orders().unwrap();
        assert_eq!(all.len(), 2);
    }
}
