use crate::types::SimulatedOrder;
use tracing::info;

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
}
