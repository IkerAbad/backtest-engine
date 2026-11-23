use crate::types::EngineConfig;

/// Result of pricing one market order.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionCost {
    /// Price actually paid or received, including half-spread and slippage.
    pub price: f64,
    /// Absolute traded notional at the reference price.
    pub notional: f64,
    pub commission: f64,
    pub spread_cost: f64,
    pub slippage_cost: f64,
}

/// Linear cost model.
///
/// * commission: `fixed + bps * notional / 10_000`
/// * spread: half of `spread_bps` is paid on each side
/// * slippage: `slippage_bps_at_full_volume * (quantity / bar_volume)`
///
/// The last term is the "proportional to volume" rule: trading 1% of a bar's
/// volume costs 1% of the full-volume impact parameter.
#[derive(Clone, Debug)]
pub struct CostModel {
    pub commission_bps: f64,
    pub commission_fixed: f64,
    pub spread_bps: f64,
    pub slippage_bps_at_full_volume: f64,
}

impl CostModel {
    pub fn from_config(config: &EngineConfig) -> Self {
        Self {
            commission_bps: config.commission_bps,
            commission_fixed: config.commission_fixed,
            spread_bps: config.spread_bps,
            slippage_bps_at_full_volume: config.slippage_bps_at_full_volume,
        }
    }

    /// Prices a signed quantity. Positive `quantity` buys, negative sells.
    pub fn execute(&self, reference_price: f64, quantity: f64, bar_volume: f64) -> ExecutionCost {
        debug_assert!(reference_price > 0.0);
        debug_assert!(bar_volume > 0.0);
        if quantity == 0.0 {
            return ExecutionCost {
                price: reference_price,
                notional: 0.0,
                commission: 0.0,
                spread_cost: 0.0,
                slippage_cost: 0.0,
            };
        }
        let side = if quantity > 0.0 { 1.0 } else { -1.0 };
        let absolute = quantity.abs();
        let notional = absolute * reference_price;
        let participation = absolute / bar_volume;
        let half_spread_bps = self.spread_bps / 2.0;
        let slippage_bps = self.slippage_bps_at_full_volume * participation;
        let price = reference_price * (1.0 + side * (half_spread_bps + slippage_bps) / 10_000.0);
        let commission = self.commission_fixed + self.commission_bps * notional / 10_000.0;
        let spread_cost = notional * half_spread_bps / 10_000.0;
        let slippage_cost = notional * slippage_bps / 10_000.0;
        ExecutionCost {
            price,
            notional,
            commission,
            spread_cost,
            slippage_cost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> CostModel {
        CostModel {
            commission_bps: 1.0,
            commission_fixed: 2.0,
            spread_bps: 10.0,
            slippage_bps_at_full_volume: 100.0,
        }
    }

    #[test]
    fn zero_trade_costs_nothing() {
        let cost = model().execute(100.0, 0.0, 1_000.0);
        assert_eq!(cost.price, 100.0);
        assert_eq!(cost.commission, 0.0);
        assert_eq!(cost.spread_cost, 0.0);
        assert_eq!(cost.slippage_cost, 0.0);
    }

    #[test]
    fn buy_pays_half_spread_and_slippage() {
        // 100 shares out of 1_000 = 10% participation -> 10 bps slippage.
        let cost = model().execute(100.0, 100.0, 1_000.0);
        assert!((cost.price - 100.15).abs() < 1e-12); // 5 bps half-spread + 10 bps slip
        assert!((cost.spread_cost - 5.0).abs() < 1e-12);
        assert!((cost.slippage_cost - 10.0).abs() < 1e-12);
        assert!((cost.commission - 3.0).abs() < 1e-12);
    }

    #[test]
    fn sell_receives_less() {
        let cost = model().execute(100.0, -100.0, 1_000.0);
        assert!((cost.price - 99.85).abs() < 1e-12);
    }

    #[test]
    fn slippage_is_linear_in_participation() {
        let small = model().execute(100.0, 10.0, 1_000.0);
        let large = model().execute(100.0, 100.0, 1_000.0);
        // Price impact is 10x larger at 10x the participation (half-spread is constant).
        let small_slip_bps = (small.price - 100.0) / 100.0 * 10_000.0 - 5.0;
        let large_slip_bps = (large.price - 100.0) / 100.0 * 10_000.0 - 5.0;
        assert!((large_slip_bps - small_slip_bps * 10.0).abs() < 1e-9);
    }

    #[test]
    fn zero_config_has_no_frictions() {
        let model = CostModel {
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
        };
        let cost = model.execute(42.0, 123.0, 10.0);
        assert_eq!(cost.price, 42.0);
        assert_eq!(cost.commission + cost.spread_cost + cost.slippage_cost, 0.0);
    }
}
