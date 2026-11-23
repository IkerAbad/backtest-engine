use crate::types::{EquityPoint, Metrics, Trade};

/// Computes the report metrics from an equity curve and a trade list.
///
/// Only IEEE-754 basic operations and `sqrt` are used, and every summation
/// has a fixed order. The values are therefore bit-identical on every
/// 64-bit platform the project supports.
pub fn compute(
    equity: &[EquityPoint],
    trades: &[Trade],
    initial_cash: f64,
    periods_per_year: u32,
) -> Metrics {
    let final_equity = equity.last().map_or(initial_cash, |point| point.equity);
    let total_return = if initial_cash > 0.0 {
        final_equity / initial_cash - 1.0
    } else {
        0.0
    };

    let mut returns = Vec::with_capacity(equity.len().saturating_sub(1));
    for window in equity.windows(2) {
        if window[0].equity > 0.0 {
            returns.push(window[1].equity / window[0].equity - 1.0);
        } else {
            returns.push(0.0);
        }
    }

    let annualized_volatility = if returns.len() >= 2 {
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let squared = returns
            .iter()
            .map(|value| {
                let delta = value - mean;
                delta * delta
            })
            .sum::<f64>();
        let variance = squared / (returns.len() - 1) as f64;
        (variance * periods_per_year as f64).sqrt()
    } else {
        0.0
    };

    let sharpe = if returns.len() >= 2 {
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let squared = returns
            .iter()
            .map(|value| {
                let delta = value - mean;
                delta * delta
            })
            .sum::<f64>();
        let variance = squared / (returns.len() - 1) as f64;
        let stddev = variance.sqrt();
        if stddev > 0.0 {
            mean / stddev * (periods_per_year as f64).sqrt()
        } else {
            0.0
        }
    } else {
        0.0
    };

    let mut peak = equity.first().map_or(initial_cash, |point| point.equity);
    let mut max_drawdown = 0.0;
    for point in equity {
        if point.equity > peak {
            peak = point.equity;
        }
        if peak > 0.0 {
            let drawdown = point.equity / peak - 1.0;
            if drawdown < max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }

    let traded_notional = trades.iter().map(|trade| trade.notional).sum::<f64>();
    let mean_equity = if equity.is_empty() {
        initial_cash
    } else {
        equity.iter().map(|point| point.equity).sum::<f64>() / equity.len() as f64
    };
    let turnover = if mean_equity > 0.0 {
        traded_notional / mean_equity
    } else {
        0.0
    };

    let commission_cost = trades.iter().map(|trade| trade.commission).sum::<f64>();
    let spread_cost = trades.iter().map(|trade| trade.spread_cost).sum::<f64>();
    let slippage_cost = trades.iter().map(|trade| trade.slippage_cost).sum::<f64>();
    let total_cost = commission_cost + spread_cost + slippage_cost;
    let cost_bps = if initial_cash > 0.0 {
        total_cost / initial_cash * 10_000.0
    } else {
        0.0
    };

    let avg_exposure = if equity.is_empty() {
        0.0
    } else {
        equity
            .iter()
            .map(|point| {
                if point.equity != 0.0 {
                    point.position_value / point.equity.abs()
                } else {
                    0.0
                }
            })
            .sum::<f64>()
            / equity.len() as f64
    };

    Metrics {
        total_return,
        annualized_volatility,
        sharpe,
        max_drawdown,
        turnover,
        total_cost,
        commission_cost,
        spread_cost,
        slippage_cost,
        cost_bps,
        final_equity,
        initial_cash,
        n_trades: trades.len(),
        n_bars: equity.len(),
        avg_exposure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: i64, equity: f64) -> EquityPoint {
        EquityPoint {
            ts,
            equity,
            cash: equity,
            position_value: 0.0,
        }
    }

    fn trade(notional: f64, commission: f64, spread: f64, slippage: f64) -> Trade {
        Trade {
            ts: 0,
            symbol: "ACME".into(),
            side: "buy".into(),
            quantity: 1.0,
            price: 1.0,
            reference_price: 1.0,
            notional,
            commission,
            spread_cost: spread,
            slippage_cost: slippage,
        }
    }

    #[test]
    fn total_return_and_drawdown() {
        let curve = [
            point(1, 100.0),
            point(2, 110.0),
            point(3, 99.0),
            point(4, 120.0),
        ];
        let metrics = compute(&curve, &[], 100.0, 252);
        assert!((metrics.total_return - 0.2).abs() < 1e-12);
        assert!((metrics.max_drawdown - (99.0 / 110.0 - 1.0)).abs() < 1e-12);
        assert_eq!(metrics.n_bars, 4);
    }

    #[test]
    fn flat_curve_has_zero_sharpe() {
        let curve = [point(1, 100.0), point(2, 100.0), point(3, 100.0)];
        let metrics = compute(&curve, &[], 100.0, 252);
        assert_eq!(metrics.sharpe, 0.0);
        assert_eq!(metrics.annualized_volatility, 0.0);
        assert_eq!(metrics.max_drawdown, 0.0);
    }

    #[test]
    fn costs_and_turnover_add_up() {
        let curve = [point(1, 100.0), point(2, 101.0)];
        let trades = [trade(1_000.0, 1.0, 0.5, 2.0), trade(500.0, 0.5, 0.25, 1.0)];
        let metrics = compute(&curve, &trades, 100.0, 252);
        assert!((metrics.commission_cost - 1.5).abs() < 1e-12);
        assert!((metrics.spread_cost - 0.75).abs() < 1e-12);
        assert!((metrics.slippage_cost - 3.0).abs() < 1e-12);
        assert!((metrics.total_cost - 5.25).abs() < 1e-12);
        assert!((metrics.turnover - 1_500.0 / 100.5).abs() < 1e-12);
        assert!((metrics.cost_bps - 525.0).abs() < 1e-12);
    }

    #[test]
    fn rising_curve_has_positive_sharpe() {
        let curve = [
            point(1, 100.0),
            point(2, 101.0),
            point(3, 102.0),
            point(4, 103.0),
        ];
        let metrics = compute(&curve, &[], 100.0, 252);
        assert!(metrics.sharpe > 0.0);
        assert!(metrics.annualized_volatility > 0.0);
    }
}
