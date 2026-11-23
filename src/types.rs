use pyo3::prelude::*;

use crate::error::EngineError;
use crate::time::{format_date, format_timestamp};

/// One daily (or intraday) OHLCV observation.
#[pyclass(name = "Bar")]
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub ts: i64,
    #[pyo3(get)]
    pub published_at: i64,
    #[pyo3(get)]
    pub version: u32,
    #[pyo3(get)]
    pub open: f64,
    #[pyo3(get)]
    pub high: f64,
    #[pyo3(get)]
    pub low: f64,
    #[pyo3(get)]
    pub close: f64,
    #[pyo3(get)]
    pub volume: f64,
}

#[pymethods]
impl Bar {
    #[getter]
    fn datetime(&self) -> String {
        format_timestamp(self.ts)
    }

    #[getter]
    fn date(&self) -> String {
        format_date(self.ts)
    }

    #[getter]
    fn published_datetime(&self) -> String {
        format_timestamp(self.published_at)
    }

    fn __repr__(&self) -> String {
        format!(
            "Bar(symbol={:?}, date={:?}, close={:.4}, version={})",
            self.symbol,
            format_date(self.ts),
            self.close,
            self.version
        )
    }
}

/// A target-weight instruction emitted by a strategy.
#[pyclass(name = "Order")]
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub target_weight: f64,
}

#[pymethods]
impl Order {
    #[new]
    fn new(symbol: String, target_weight: f64) -> Self {
        Self {
            symbol,
            target_weight,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Order({:?}, target_weight={:.6})",
            self.symbol, self.target_weight
        )
    }
}

/// One executed fill.
#[pyclass(name = "Trade")]
#[derive(Clone, Debug)]
pub struct Trade {
    #[pyo3(get)]
    pub ts: i64,
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub side: String,
    #[pyo3(get)]
    pub quantity: f64,
    #[pyo3(get)]
    pub price: f64,
    #[pyo3(get)]
    pub reference_price: f64,
    #[pyo3(get)]
    pub notional: f64,
    #[pyo3(get)]
    pub commission: f64,
    #[pyo3(get)]
    pub spread_cost: f64,
    #[pyo3(get)]
    pub slippage_cost: f64,
}

#[pymethods]
impl Trade {
    #[getter]
    fn date(&self) -> String {
        format_date(self.ts)
    }

    #[getter]
    fn total_cost(&self) -> f64 {
        self.commission + self.spread_cost + self.slippage_cost
    }

    fn __repr__(&self) -> String {
        format!(
            "Trade({}, {:?}, {} {:.4} @ {:.4})",
            format_date(self.ts),
            self.symbol,
            self.side,
            self.quantity,
            self.price
        )
    }
}

/// One point of the mark-to-market equity curve.
#[pyclass(name = "EquityPoint")]
#[derive(Clone, Debug)]
pub struct EquityPoint {
    #[pyo3(get)]
    pub ts: i64,
    #[pyo3(get)]
    pub equity: f64,
    #[pyo3(get)]
    pub cash: f64,
    #[pyo3(get)]
    pub position_value: f64,
}

#[pymethods]
impl EquityPoint {
    #[getter]
    fn date(&self) -> String {
        format_date(self.ts)
    }
}

/// The orders a strategy emitted for one delivered bar. Empty decisions are
/// recorded too, so two runs can be compared event by event.
#[pyclass(name = "Decision")]
#[derive(Clone, Debug)]
pub struct Decision {
    #[pyo3(get)]
    pub ts: i64,
    #[pyo3(get)]
    pub symbol: String,
    pub orders: Vec<Order>,
}

#[pymethods]
impl Decision {
    #[getter]
    fn date(&self) -> String {
        format_date(self.ts)
    }

    #[getter]
    fn orders(&self) -> Vec<Order> {
        self.orders.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "Decision({}, {:?}, {} order(s))",
            format_date(self.ts),
            self.symbol,
            self.orders.len()
        )
    }
}

/// Record of one point-in-time read. `future` is true when the strategy asked
/// for a timestamp after the simulated clock.
#[pyclass(name = "DataAccess")]
#[derive(Clone, Debug)]
pub struct DataAccess {
    #[pyo3(get)]
    pub clock: i64,
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub requested_ts: i64,
    #[pyo3(get)]
    pub effective_ts: Option<i64>,
    #[pyo3(get)]
    pub returned_ts: Option<i64>,
    #[pyo3(get)]
    pub future: bool,
    #[pyo3(get)]
    pub kind: String,
}

#[pymethods]
impl DataAccess {
    #[getter]
    fn clock_date(&self) -> String {
        format_date(self.clock)
    }

    #[getter]
    fn requested_date(&self) -> String {
        format_date(self.requested_ts)
    }

    #[getter]
    fn returned_date(&self) -> Option<String> {
        self.returned_ts.map(format_date)
    }

    fn __repr__(&self) -> String {
        format!(
            "DataAccess({}, {:?}, requested={}, future={})",
            format_date(self.clock),
            self.symbol,
            format_date(self.requested_ts),
            self.future
        )
    }
}

/// Engine configuration. The defaults describe a small daily equity backtest
/// with realistic but configurable frictions.
#[pyclass(name = "EngineConfig")]
#[derive(Clone, Debug)]
pub struct EngineConfig {
    #[pyo3(get)]
    pub initial_cash: f64,
    #[pyo3(get)]
    pub commission_bps: f64,
    #[pyo3(get)]
    pub commission_fixed: f64,
    #[pyo3(get)]
    pub spread_bps: f64,
    #[pyo3(get)]
    pub slippage_bps_at_full_volume: f64,
    #[pyo3(get)]
    pub periods_per_year: u32,
    #[pyo3(get)]
    pub start_ts: Option<i64>,
    #[pyo3(get)]
    pub end_ts: Option<i64>,
    #[pyo3(get)]
    pub masked: bool,
    #[pyo3(get)]
    pub strict_future_access: bool,
}

#[pymethods]
impl EngineConfig {
    #[new]
    #[pyo3(signature = (
        initial_cash = 1_000_000.0,
        commission_bps = 1.0,
        commission_fixed = 0.0,
        spread_bps = 2.0,
        slippage_bps_at_full_volume = 100.0,
        periods_per_year = 252,
        start_ts = None,
        end_ts = None,
        masked = false,
        strict_future_access = true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        initial_cash: f64,
        commission_bps: f64,
        commission_fixed: f64,
        spread_bps: f64,
        slippage_bps_at_full_volume: f64,
        periods_per_year: u32,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        masked: bool,
        strict_future_access: bool,
    ) -> Self {
        Self {
            initial_cash,
            commission_bps,
            commission_fixed,
            spread_bps,
            slippage_bps_at_full_volume,
            periods_per_year,
            start_ts,
            end_ts,
            masked,
            strict_future_access,
        }
    }

    /// Returns a copy with the masking and strictness flags overridden.
    #[pyo3(signature = (masked = None, strict_future_access = None))]
    fn variant(&self, masked: Option<bool>, strict_future_access: Option<bool>) -> Self {
        let mut out = self.clone();
        if let Some(value) = masked {
            out.masked = value;
        }
        if let Some(value) = strict_future_access {
            out.strict_future_access = value;
        }
        out
    }

    /// Returns a copy restricted to `[start_ts, end_ts)`.
    fn with_window(&self, start_ts: Option<i64>, end_ts: Option<i64>) -> Self {
        let mut out = self.clone();
        out.start_ts = start_ts;
        out.end_ts = end_ts;
        out
    }

    /// Returns a copy with every trading cost set to zero.
    fn without_costs(&self) -> Self {
        let mut out = self.clone();
        out.commission_bps = 0.0;
        out.commission_fixed = 0.0;
        out.spread_bps = 0.0;
        out.slippage_bps_at_full_volume = 0.0;
        out
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        if !self.initial_cash.is_finite() || self.initial_cash <= 0.0 {
            return Err(EngineError::Config(
                "initial_cash must be finite and greater than zero".into(),
            ));
        }
        for (name, value) in [
            ("commission_bps", self.commission_bps),
            ("commission_fixed", self.commission_fixed),
            ("spread_bps", self.spread_bps),
            (
                "slippage_bps_at_full_volume",
                self.slippage_bps_at_full_volume,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(EngineError::Config(format!(
                    "{name} must be finite and non-negative"
                )));
            }
        }
        if self.periods_per_year == 0 {
            return Err(EngineError::Config(
                "periods_per_year must be greater than zero".into(),
            ));
        }
        if let (Some(start), Some(end)) = (self.start_ts, self.end_ts) {
            if start >= end {
                return Err(EngineError::Config(
                    "start_ts must be earlier than end_ts".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Performance and cost summary for one run.
#[pyclass(name = "Metrics")]
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    #[pyo3(get)]
    pub total_return: f64,
    #[pyo3(get)]
    pub annualized_volatility: f64,
    #[pyo3(get)]
    pub sharpe: f64,
    #[pyo3(get)]
    pub max_drawdown: f64,
    #[pyo3(get)]
    pub turnover: f64,
    #[pyo3(get)]
    pub total_cost: f64,
    #[pyo3(get)]
    pub commission_cost: f64,
    #[pyo3(get)]
    pub spread_cost: f64,
    #[pyo3(get)]
    pub slippage_cost: f64,
    #[pyo3(get)]
    pub cost_bps: f64,
    #[pyo3(get)]
    pub final_equity: f64,
    #[pyo3(get)]
    pub initial_cash: f64,
    #[pyo3(get)]
    pub n_trades: usize,
    #[pyo3(get)]
    pub n_bars: usize,
    #[pyo3(get)]
    pub avg_exposure: f64,
}

/// Everything a run produced, in deterministic order.
#[pyclass(name = "BacktestResult")]
#[derive(Clone, Debug)]
pub struct BacktestResult {
    pub metrics: Metrics,
    pub equity: Vec<EquityPoint>,
    pub trades: Vec<Trade>,
    pub decisions: Vec<Decision>,
    pub data_accesses: Vec<DataAccess>,
    pub unfilled_orders: Vec<Order>,
    pub config: EngineConfig,
    #[pyo3(get)]
    pub initial_cash: f64,
    #[pyo3(get)]
    pub final_equity: f64,
    #[pyo3(get)]
    pub data_hash: String,
    #[pyo3(get)]
    pub data_rows: usize,
    #[pyo3(get)]
    pub data_revisions: usize,
    #[pyo3(get)]
    pub data_start_ts: Option<i64>,
    #[pyo3(get)]
    pub data_end_ts: Option<i64>,
    #[pyo3(get)]
    pub bars_processed: usize,
    pub symbols: Vec<String>,
}

#[pymethods]
impl BacktestResult {
    #[getter]
    fn metrics(&self) -> Metrics {
        self.metrics.clone()
    }

    #[getter]
    fn equity(&self) -> Vec<EquityPoint> {
        self.equity.clone()
    }

    #[getter]
    fn trades(&self) -> Vec<Trade> {
        self.trades.clone()
    }

    #[getter]
    fn decisions(&self) -> Vec<Decision> {
        self.decisions.clone()
    }

    #[getter]
    fn data_accesses(&self) -> Vec<DataAccess> {
        self.data_accesses.clone()
    }

    #[getter]
    fn unfilled_orders(&self) -> Vec<Order> {
        self.unfilled_orders.clone()
    }

    #[getter]
    fn config(&self) -> EngineConfig {
        self.config.clone()
    }

    #[getter]
    fn symbols(&self) -> Vec<String> {
        self.symbols.clone()
    }

    /// Renders the self-contained, reproducible HTML report.
    fn render_html(&self, strategy_name: &str, title: &str, lookahead_verdict: &str) -> String {
        crate::report::render(self, strategy_name, title, lookahead_verdict)
    }

    fn __repr__(&self) -> String {
        format!(
            "BacktestResult(trades={}, final_equity={:.2}, return={:.4}, lookahead_reads={})",
            self.trades.len(),
            self.final_equity,
            self.metrics.total_return,
            self.data_accesses.iter().filter(|a| a.future).count()
        )
    }
}
