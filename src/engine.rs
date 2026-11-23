use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pyo3::prelude::*;

use crate::costs::CostModel;
use crate::error::EngineError;
use crate::metrics;
use crate::queue::EventQueue;
use crate::store::PointInTimeStore;
use crate::time::{format_date, format_timestamp, parse_timestamp};
use crate::types::{
    BacktestResult, Bar, DataAccess, Decision, EngineConfig, EquityPoint, Order, Trade,
};

/// A strategy receives the simulated clock and one bar at a time. It never
/// receives the dataset itself, only a point-in-time view.
pub trait Strategy {
    fn on_start(&mut self, _ctx: &Context) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_bar(&mut self, ctx: &Context, bar: &Bar) -> Result<Vec<Order>, EngineError>;

    fn on_finish(&mut self, _ctx: &Context) -> Result<(), EngineError> {
        Ok(())
    }
}

/// The only data handle a strategy can touch.
#[pyclass(name = "Context")]
#[derive(Clone)]
pub struct Context {
    pub clock: i64,
    pub store: Arc<PointInTimeStore>,
    pub cash: f64,
    pub equity: f64,
    pub positions: BTreeMap<String, f64>,
    pub masked: bool,
    pub strict: bool,
    pub access_log: Arc<Mutex<Vec<DataAccess>>>,
}

impl Context {
    /// Point-in-time read. In a normal run, asking beyond the clock is a
    /// deliberate look-ahead attempt and is rejected when `strict` is on. In a
    /// masked run the request returns `None`, which is how the detector
    /// replays the same strategy without a future.
    pub fn as_of(&self, symbol: &str, requested: i64) -> Result<Option<Bar>, EngineError> {
        let future = requested > self.clock;
        if future && self.strict && !self.masked {
            self.log_access(symbol, requested, None, None, future, "as_of");
            return Err(EngineError::FutureAccess {
                symbol: symbol.to_string(),
                clock: self.clock,
                requested,
            });
        }
        let returned = if future && self.masked {
            None
        } else {
            self.store.as_of(symbol, requested)
        };
        self.log_access(
            symbol,
            requested,
            if future && self.masked {
                None
            } else {
                Some(requested)
            },
            returned.as_ref().map(|bar| bar.ts),
            future,
            "as_of",
        );
        Ok(returned)
    }

    /// Raw research view: the entire series, including future rows and future
    /// revisions. Strict mode rejects it. The detector's masked run truncates
    /// it at the clock, so a strategy that normalizes with full-sample
    /// statistics is caught.
    pub fn research_series(&self, symbol: &str) -> Result<Vec<Bar>, EngineError> {
        let all = self.store.latest_series(symbol);
        let requested = all.last().map_or(self.clock, |bar| bar.ts);
        let future = all
            .iter()
            .any(|bar| bar.ts > self.clock || bar.published_at > self.clock);
        if self.strict && !self.masked {
            self.log_access(symbol, requested, None, None, future, "research_series");
            return Err(EngineError::FutureAccess {
                symbol: symbol.to_string(),
                clock: self.clock,
                requested,
            });
        }
        let returned = if self.masked {
            self.store.series_as_of(symbol, self.clock)
        } else {
            all
        };
        self.log_access(
            symbol,
            requested,
            if self.masked { Some(self.clock) } else { None },
            returned.last().map(|bar| bar.ts),
            future,
            "research_series",
        );
        Ok(returned)
    }

    fn log_access(
        &self,
        symbol: &str,
        requested: i64,
        effective: Option<i64>,
        returned: Option<i64>,
        future: bool,
        kind: &str,
    ) {
        self.access_log
            .lock()
            .expect("access log mutex poisoned")
            .push(DataAccess {
                clock: self.clock,
                symbol: symbol.to_string(),
                requested_ts: requested,
                effective_ts: effective,
                returned_ts: returned,
                future,
                kind: kind.into(),
            });
    }

    pub fn history(&self, symbol: &str, limit: usize) -> Vec<Bar> {
        self.store.history_before(symbol, self.clock, limit)
    }

    pub fn position_of(&self, symbol: &str) -> f64 {
        *self.positions.get(symbol).unwrap_or(&0.0)
    }

    pub fn price(&self, symbol: &str) -> Option<f64> {
        self.store.as_of(symbol, self.clock).map(|bar| bar.close)
    }
}

#[pymethods]
impl Context {
    #[getter]
    fn now(&self) -> i64 {
        self.clock
    }

    #[getter]
    fn datetime(&self) -> String {
        format_timestamp(self.clock)
    }

    #[getter]
    fn date(&self) -> String {
        format_date(self.clock)
    }

    #[getter]
    fn equity(&self) -> f64 {
        self.equity
    }

    #[getter]
    fn cash(&self) -> f64 {
        self.cash
    }

    #[getter]
    fn masked(&self) -> bool {
        self.masked
    }

    #[getter]
    fn strict_future_access(&self) -> bool {
        self.strict
    }

    fn position(&self, symbol: &str) -> f64 {
        self.position_of(symbol)
    }

    fn positions(&self) -> Vec<(String, f64)> {
        self.positions
            .iter()
            .map(|(symbol, quantity)| (symbol.clone(), *quantity))
            .collect()
    }

    #[pyo3(name = "price")]
    fn py_price(&self, symbol: &str) -> Option<f64> {
        Context::price(self, symbol)
    }

    #[pyo3(name = "history")]
    fn py_history(&self, symbol: &str, limit: usize) -> Vec<Bar> {
        self.store.history_before(symbol, self.clock, limit)
    }

    #[pyo3(name = "as_of")]
    fn py_as_of(&self, symbol: &str, ts: i64) -> PyResult<Option<Bar>> {
        self.as_of(symbol, ts).map_err(PyErr::from)
    }

    #[pyo3(name = "as_of_iso")]
    fn py_as_of_iso(&self, symbol: &str, ts: &str) -> PyResult<Option<Bar>> {
        let parsed = parse_timestamp(ts).map_err(pyo3::exceptions::PyValueError::new_err)?;
        self.as_of(symbol, parsed).map_err(PyErr::from)
    }

    /// Returns the whole stored series for `symbol`, including future rows and
    /// future revisions. This is a research escape hatch that models handing a
    /// full DataFrame to a strategy. Strict runs reject it. The detector's
    /// masked run truncates it at the clock.
    #[pyo3(name = "research_series")]
    fn py_research_series(&self, symbol: &str) -> PyResult<Vec<Bar>> {
        self.research_series(symbol).map_err(PyErr::from)
    }

    fn order(&self, symbol: &str, target_weight: f64) -> PyResult<Order> {
        if symbol.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "order symbol must not be empty",
            ));
        }
        if !target_weight.is_finite() || !(-1.0..=1.0).contains(&target_weight) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "target_weight must be finite and in [-1, 1]",
            ));
        }
        Ok(Order {
            symbol: symbol.to_string(),
            target_weight,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Context(clock={}, equity={:.2}, positions={})",
            format_date(self.clock),
            self.equity,
            self.positions.len()
        )
    }
}

#[derive(Clone, Debug)]
struct Portfolio {
    cash: f64,
    positions: BTreeMap<String, f64>,
    last_close: BTreeMap<String, f64>,
}

impl Portfolio {
    fn new(initial_cash: f64) -> Self {
        Self {
            cash: initial_cash,
            positions: BTreeMap::new(),
            last_close: BTreeMap::new(),
        }
    }

    fn mark(&mut self, symbol: &str, close: f64) {
        self.last_close.insert(symbol.to_string(), close);
    }

    fn position(&self, symbol: &str) -> f64 {
        *self.positions.get(symbol).unwrap_or(&0.0)
    }

    fn equity(&self) -> f64 {
        let mut value = self.cash;
        for (symbol, quantity) in &self.positions {
            if let Some(close) = self.last_close.get(symbol) {
                value += quantity * close;
            }
        }
        value
    }

    fn gross_position_value(&self) -> f64 {
        let mut value = 0.0;
        for (symbol, quantity) in &self.positions {
            if let Some(close) = self.last_close.get(symbol) {
                value += (quantity * close).abs();
            }
        }
        value
    }

    fn apply_fill(&mut self, symbol: &str, quantity: f64, price: f64, commission: f64) {
        let position = self.positions.entry(symbol.to_string()).or_insert(0.0);
        *position += quantity;
        self.cash -= quantity * price + commission;
    }
}

#[derive(Clone, Debug)]
struct PendingOrder {
    symbol: String,
    quantity: f64,
    target_weight: f64,
}

fn snapshot_context(
    clock: i64,
    store: &Arc<PointInTimeStore>,
    portfolio: &Portfolio,
    config: &EngineConfig,
    access_log: &Arc<Mutex<Vec<DataAccess>>>,
) -> Context {
    Context {
        clock,
        store: Arc::clone(store),
        cash: portfolio.cash,
        equity: portfolio.equity(),
        positions: portfolio.positions.clone(),
        masked: config.masked,
        strict: config.strict_future_access,
        access_log: Arc::clone(access_log),
    }
}

/// A strict strategy can catch `FutureAccessError` in Python. The access log
/// still records the attempt, so the engine checks the log after every
/// callback and aborts even if the strategy swallowed the exception.
fn reject_logged_future_reads(
    access_log: &Arc<Mutex<Vec<DataAccess>>>,
    start: usize,
    config: &EngineConfig,
) -> Result<(), EngineError> {
    if !config.strict_future_access || config.masked {
        return Ok(());
    }
    let log = access_log.lock().expect("access log mutex poisoned");
    if let Some(access) = log.iter().skip(start).find(|access| access.future) {
        return Err(EngineError::FutureAccess {
            symbol: access.symbol.clone(),
            clock: access.clock,
            requested: access.requested_ts,
        });
    }
    Ok(())
}

/// Runs one backtest.
///
/// Events are consumed in `(timestamp, symbol)` order. Orders produced on a
/// bar execute at the next bar's open for that symbol, never inside the bar
/// that produced the signal.
pub fn run(
    strategy: &mut dyn Strategy,
    store: Arc<PointInTimeStore>,
    config: &EngineConfig,
) -> Result<BacktestResult, EngineError> {
    config.validate()?;

    let mut queue = EventQueue::new();
    for bar in store.events_in_range(config.start_ts, config.end_ts) {
        queue.push(bar);
    }

    let access_log = Arc::new(Mutex::new(Vec::new()));
    let mut portfolio = Portfolio::new(config.initial_cash);
    let cost_model = CostModel::from_config(config);
    let mut pending: BTreeMap<String, Vec<PendingOrder>> = BTreeMap::new();
    let mut decisions: Vec<Decision> = Vec::new();
    let mut trades: Vec<Trade> = Vec::new();
    let mut equity: Vec<EquityPoint> = Vec::new();

    let start_clock = queue
        .peek_ts()
        .map(|ts| ts - 1)
        .or(config.start_ts)
        .unwrap_or(0);
    let start_context = snapshot_context(start_clock, &store, &portfolio, config, &access_log);
    let start_log = access_log.lock().expect("access log mutex poisoned").len();
    strategy.on_start(&start_context)?;
    reject_logged_future_reads(&access_log, start_log, config)?;

    let mut clock = start_clock;
    while let Some(batch_ts) = queue.peek_ts() {
        clock = batch_ts;
        let mut batch: Vec<Bar> = Vec::new();
        while queue.peek_ts() == Some(batch_ts) {
            batch.push(queue.pop().expect("queue reported a timestamp"));
        }

        // 1. Orders from the previous bar for each symbol fill at this open.
        for bar in &batch {
            if let Some(orders) = pending.remove(&bar.symbol) {
                for order in orders {
                    if order.quantity == 0.0 || !order.quantity.is_finite() {
                        continue;
                    }
                    let execution = cost_model.execute(bar.open, order.quantity, bar.volume);
                    portfolio.apply_fill(
                        &order.symbol,
                        order.quantity,
                        execution.price,
                        execution.commission,
                    );
                    trades.push(Trade {
                        ts: bar.ts,
                        symbol: order.symbol.clone(),
                        side: if order.quantity > 0.0 {
                            "buy".into()
                        } else {
                            "sell".into()
                        },
                        quantity: order.quantity.abs(),
                        price: execution.price,
                        reference_price: bar.open,
                        notional: execution.notional,
                        commission: execution.commission,
                        spread_cost: execution.spread_cost,
                        slippage_cost: execution.slippage_cost,
                    });
                }
            }
        }

        // 2. Mark all simultaneous closes before strategies decide.
        for bar in &batch {
            portfolio.mark(&bar.symbol, bar.close);
        }

        let portfolio_equity = portfolio.equity();

        // 3. Deliver bars one by one and collect target-weight orders.
        for bar in &batch {
            let context = snapshot_context(clock, &store, &portfolio, config, &access_log);
            let log_start = access_log.lock().expect("access log mutex poisoned").len();
            let emitted = strategy.on_bar(&context, bar)?;
            reject_logged_future_reads(&access_log, log_start, config)?;
            let normalized = normalize_orders(emitted)?;
            decisions.push(Decision {
                ts: bar.ts,
                symbol: bar.symbol.clone(),
                orders: normalized.clone(),
            });
            for order in normalized {
                let price = if order.symbol == bar.symbol {
                    bar.close
                } else {
                    context.price(&order.symbol).ok_or_else(|| {
                        EngineError::Strategy(format!(
                            "order for unknown symbol {:?}",
                            order.symbol
                        ))
                    })?
                };
                if !price.is_finite() || price <= 0.0 {
                    return Err(EngineError::Strategy(format!(
                        "cannot size order for {:?}: invalid price",
                        order.symbol
                    )));
                }
                let current = portfolio.position(&order.symbol);
                let target_value = order.target_weight * portfolio_equity;
                let quantity = (target_value - current * price) / price;
                if quantity.is_finite() && quantity.abs() > 1e-12 {
                    pending
                        .entry(order.symbol.clone())
                        .or_default()
                        .push(PendingOrder {
                            symbol: order.symbol.clone(),
                            quantity,
                            target_weight: order.target_weight,
                        });
                }
            }
        }

        // 4. One equity observation per event timestamp.
        equity.push(EquityPoint {
            ts: clock,
            equity: portfolio.equity(),
            cash: portfolio.cash,
            position_value: portfolio.gross_position_value(),
        });
    }

    let finish_context = snapshot_context(clock, &store, &portfolio, config, &access_log);
    let finish_log = access_log.lock().expect("access log mutex poisoned").len();
    strategy.on_finish(&finish_context)?;
    reject_logged_future_reads(&access_log, finish_log, config)?;

    let unfilled_orders = pending
        .into_values()
        .flatten()
        .map(|order| Order {
            symbol: order.symbol,
            target_weight: order.target_weight,
        })
        .collect();

    let metrics = metrics::compute(
        &equity,
        &trades,
        config.initial_cash,
        config.periods_per_year,
    );
    let data_accesses = access_log
        .lock()
        .expect("access log mutex poisoned")
        .clone();
    let bars_processed = decisions.len();

    Ok(BacktestResult {
        metrics: metrics.clone(),
        equity,
        trades,
        decisions,
        data_accesses,
        unfilled_orders,
        config: config.clone(),
        initial_cash: config.initial_cash,
        final_equity: metrics.final_equity,
        data_hash: store.data_hash().to_string(),
        data_rows: store.row_count(),
        data_revisions: store.revision_count(),
        data_start_ts: store.first_ts(),
        data_end_ts: store.last_ts(),
        bars_processed,
        symbols: store.symbols().to_vec(),
    })
}

fn normalize_orders(orders: Vec<Order>) -> Result<Vec<Order>, EngineError> {
    let mut by_symbol: BTreeMap<String, f64> = BTreeMap::new();
    for order in orders {
        if order.symbol.trim().is_empty() {
            return Err(EngineError::Strategy("order with empty symbol".into()));
        }
        if !order.target_weight.is_finite() || !(-1.0..=1.0).contains(&order.target_weight) {
            return Err(EngineError::Strategy(format!(
                "order for {:?} has target_weight {} outside [-1, 1]",
                order.symbol, order.target_weight
            )));
        }
        // Last instruction for a symbol wins and the output is sorted, so the
        // decision trace does not depend on list order.
        by_symbol.insert(order.symbol, order.target_weight);
    }
    Ok(by_symbol
        .into_iter()
        .map(|(symbol, target_weight)| Order {
            symbol,
            target_weight,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::DAY;

    fn bar(ts: i64, open: f64, close: f64, version: u32, published_at: i64) -> Bar {
        Bar {
            symbol: "ACME".into(),
            ts,
            published_at,
            version,
            open,
            high: open.max(close) * 1.01,
            low: open.min(close) * 0.99,
            close,
            volume: 1_000_000.0,
        }
    }

    fn dataset() -> Vec<Bar> {
        vec![
            bar(DAY, 10.0, 10.0, 1, DAY),
            bar(2 * DAY, 10.0, 11.0, 1, 2 * DAY),
            bar(3 * DAY, 11.0, 12.0, 1, 3 * DAY),
            bar(4 * DAY, 12.0, 12.0, 1, 4 * DAY),
        ]
    }

    struct BuyAndHold;

    impl Strategy for BuyAndHold {
        fn on_bar(&mut self, _ctx: &Context, bar: &Bar) -> Result<Vec<Order>, EngineError> {
            Ok(vec![Order {
                symbol: bar.symbol.clone(),
                target_weight: 1.0,
            }])
        }
    }

    struct Cheater;

    impl Strategy for Cheater {
        fn on_bar(&mut self, ctx: &Context, bar: &Bar) -> Result<Vec<Order>, EngineError> {
            let _ = ctx.as_of(&bar.symbol, ctx.clock + DAY)?;
            Ok(Vec::new())
        }
    }

    struct NeverTrades;

    impl Strategy for NeverTrades {
        fn on_bar(&mut self, _ctx: &Context, _bar: &Bar) -> Result<Vec<Order>, EngineError> {
            Ok(Vec::new())
        }
    }

    struct SwallowsTheError;

    impl Strategy for SwallowsTheError {
        fn on_bar(&mut self, ctx: &Context, bar: &Bar) -> Result<Vec<Order>, EngineError> {
            let _ = ctx.as_of(&bar.symbol, ctx.clock + DAY);
            Ok(Vec::new())
        }
    }

    struct ResearchCheater;

    impl Strategy for ResearchCheater {
        fn on_bar(&mut self, ctx: &Context, bar: &Bar) -> Result<Vec<Order>, EngineError> {
            let _ = ctx.research_series(&bar.symbol);
            Ok(Vec::new())
        }
    }

    #[test]
    fn orders_fill_at_next_open() {
        let store = Arc::new(PointInTimeStore::from_rows(dataset()).unwrap());
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
            periods_per_year: 252,
            start_ts: None,
            end_ts: None,
            masked: false,
            strict_future_access: true,
        };
        let result = run(&mut BuyAndHold, store, &config).unwrap();
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].ts, 2 * DAY);
        assert_eq!(result.trades[0].reference_price, 10.0);
        // 1,000,000 at 10 -> 100,000 shares, marked at 12 on the last bar.
        assert!((result.final_equity - 1_200_000.0).abs() < 1e-6);
        assert_eq!(result.decisions.len(), 4);
        // The first decision cannot fill on its own bar.
        assert_eq!(result.equity[0].equity, 1_000_000.0);
    }

    #[test]
    fn strict_mode_rejects_future_reads() {
        let store = Arc::new(PointInTimeStore::from_rows(dataset()).unwrap());
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
            periods_per_year: 252,
            start_ts: None,
            end_ts: None,
            masked: false,
            strict_future_access: true,
        };
        let error = run(&mut Cheater, store, &config).unwrap_err();
        assert!(matches!(error, EngineError::FutureAccess { .. }));
    }

    #[test]
    fn strict_mode_rejects_even_swallowed_future_reads() {
        let store = Arc::new(PointInTimeStore::from_rows(dataset()).unwrap());
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
            periods_per_year: 252,
            start_ts: None,
            end_ts: None,
            masked: false,
            strict_future_access: true,
        };
        let error = run(&mut SwallowsTheError, store, &config).unwrap_err();
        assert!(matches!(error, EngineError::FutureAccess { .. }));
    }

    #[test]
    fn strict_mode_rejects_research_series() {
        let store = Arc::new(PointInTimeStore::from_rows(dataset()).unwrap());
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
            periods_per_year: 252,
            start_ts: None,
            end_ts: None,
            masked: false,
            strict_future_access: true,
        };
        let error = run(&mut ResearchCheater, store, &config).unwrap_err();
        assert!(matches!(error, EngineError::FutureAccess { .. }));
    }

    #[test]
    fn masked_mode_hides_the_future_without_failing() {
        let store = Arc::new(PointInTimeStore::from_rows(dataset()).unwrap());
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
            periods_per_year: 252,
            start_ts: None,
            end_ts: None,
            masked: true,
            strict_future_access: false,
        };
        let result = run(&mut Cheater, store, &config).unwrap();
        assert!(result.data_accesses.iter().all(|access| access.future));
        assert!(
            result
                .data_accesses
                .iter()
                .all(|access| access.returned_ts.is_none())
        );
    }

    #[test]
    fn input_row_order_does_not_change_the_run() {
        let rows = dataset();
        let mut reversed = rows.clone();
        reversed.reverse();
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 1.0,
            commission_fixed: 0.0,
            spread_bps: 2.0,
            slippage_bps_at_full_volume: 100.0,
            periods_per_year: 252,
            start_ts: None,
            end_ts: None,
            masked: false,
            strict_future_access: true,
        };
        let a = run(
            &mut BuyAndHold,
            Arc::new(PointInTimeStore::from_rows(rows).unwrap()),
            &config,
        )
        .unwrap();
        let b = run(
            &mut BuyAndHold,
            Arc::new(PointInTimeStore::from_rows(reversed).unwrap()),
            &config,
        )
        .unwrap();
        assert_eq!(a.data_hash, b.data_hash);
        assert_eq!(a.final_equity, b.final_equity);
        assert_eq!(a.trades.len(), b.trades.len());
        assert_eq!(a.metrics.sharpe, b.metrics.sharpe);
    }

    #[test]
    fn empty_window_is_valid() {
        let store = Arc::new(PointInTimeStore::from_rows(dataset()).unwrap());
        let config = EngineConfig {
            initial_cash: 1_000_000.0,
            commission_bps: 0.0,
            commission_fixed: 0.0,
            spread_bps: 0.0,
            slippage_bps_at_full_volume: 0.0,
            periods_per_year: 252,
            start_ts: Some(10 * DAY),
            end_ts: Some(11 * DAY),
            masked: false,
            strict_future_access: true,
        };
        let result = run(&mut NeverTrades, store, &config).unwrap();
        assert_eq!(result.metrics.n_bars, 0);
        assert_eq!(result.final_equity, config.initial_cash);
    }
}
