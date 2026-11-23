//! Python bindings for the backtest engine.
//!
//! The Rust core owns the event queue, the point-in-time store, execution
//! costs, metrics and the HTML report. Python only supplies the strategy.

mod costs;
mod engine;
mod error;
mod metrics;
mod queue;
mod report;
mod store;
mod time;
mod types;

use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use sha2::{Digest, Sha256};

use crate::engine::{Context, Strategy};
use crate::error::EngineError;
use crate::store::PointInTimeStore;
use crate::types::{BacktestResult, Bar, EngineConfig, Order};

create_exception!(_backtest_engine, FutureAccessError, PyRuntimeError);

impl From<EngineError> for PyErr {
    fn from(error: EngineError) -> Self {
        let message = error.message();
        match error {
            EngineError::FutureAccess { .. } => FutureAccessError::new_err(message),
            _ => PyRuntimeError::new_err(message),
        }
    }
}

struct PyStrategyAdapter {
    object: Py<PyAny>,
}

fn strategy_error(error: PyErr) -> EngineError {
    EngineError::Strategy(error.to_string())
}

fn future_access_error(ctx: &Context, error: PyErr) -> EngineError {
    let last = ctx
        .access_log
        .lock()
        .expect("access log mutex poisoned")
        .iter()
        .rev()
        .find(|access| access.future)
        .cloned();
    match last {
        Some(access) => EngineError::FutureAccess {
            symbol: access.symbol,
            clock: access.clock,
            requested: access.requested_ts,
        },
        None => strategy_error(error),
    }
}

impl PyStrategyAdapter {
    fn lifecycle(&self, name: &str, context: &Context) -> Result<(), EngineError> {
        Python::attach(|py| {
            let object = self.object.bind(py);
            if object.hasattr(name).map_err(strategy_error)? {
                let py_context = Py::new(py, context.clone()).map_err(strategy_error)?;
                if let Err(error) = object.call_method1(name, (py_context,)) {
                    if error.is_instance_of::<FutureAccessError>(py) {
                        return Err(future_access_error(context, error));
                    }
                    return Err(strategy_error(error));
                }
            }
            Ok(())
        })
    }
}

impl Strategy for PyStrategyAdapter {
    fn on_start(&mut self, ctx: &Context) -> Result<(), EngineError> {
        self.lifecycle("on_start", ctx)
    }

    fn on_bar(&mut self, ctx: &Context, bar: &Bar) -> Result<Vec<Order>, EngineError> {
        Python::attach(|py| {
            let object = self.object.bind(py);
            let context = Py::new(py, ctx.clone()).map_err(strategy_error)?;
            let bar = Py::new(py, bar.clone()).map_err(strategy_error)?;
            let value = match object.call_method1("on_bar", (context, bar)) {
                Ok(value) => value,
                Err(error) => {
                    if error.is_instance_of::<FutureAccessError>(py) {
                        return Err(future_access_error(ctx, error));
                    }
                    return Err(strategy_error(error));
                }
            };
            if value.is_none() {
                return Ok(Vec::new());
            }
            let mut orders = Vec::new();
            let iterator = value.try_iter().map_err(strategy_error)?;
            for item in iterator {
                let item = item.map_err(strategy_error)?;
                let symbol: String = item
                    .getattr("symbol")
                    .map_err(strategy_error)?
                    .extract()
                    .map_err(strategy_error)?;
                let target_weight: f64 = item
                    .getattr("target_weight")
                    .map_err(strategy_error)?
                    .extract()
                    .map_err(strategy_error)?;
                orders.push(Order {
                    symbol,
                    target_weight,
                });
            }
            Ok(orders)
        })
    }

    fn on_finish(&mut self, ctx: &Context) -> Result<(), EngineError> {
        self.lifecycle("on_finish", ctx)
    }
}

#[pyfunction]
fn run_backtest(
    strategy: Py<PyAny>,
    store: &PointInTimeStore,
    config: &EngineConfig,
) -> PyResult<BacktestResult> {
    Python::attach(|py| {
        if !strategy.bind(py).hasattr("on_bar")? {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "strategy must define on_bar(ctx, bar)",
            ));
        }
        Ok(())
    })?;
    let store = Arc::new(store.clone());
    let mut adapter = PyStrategyAdapter { object: strategy };
    engine::run(&mut adapter, store, config).map_err(PyErr::from)
}

/// SHA-256 of a UTF-8 string, as lowercase hexadecimal.
#[pyfunction]
fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    store::to_hex(&digest)
}

/// Engine version compiled into the extension.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn _backtest_engine(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Bar>()?;
    module.add_class::<Context>()?;
    module.add_class::<crate::types::DataAccess>()?;
    module.add_class::<crate::types::Decision>()?;
    module.add_class::<EngineConfig>()?;
    module.add_class::<crate::types::EquityPoint>()?;
    module.add_class::<crate::types::Metrics>()?;
    module.add_class::<Order>()?;
    module.add_class::<PointInTimeStore>()?;
    module.add_class::<BacktestResult>()?;
    module.add_class::<crate::types::Trade>()?;
    module.add_function(wrap_pyfunction!(run_backtest, module)?)?;
    module.add_function(wrap_pyfunction!(sha256_hex, module)?)?;
    module.add_function(wrap_pyfunction!(version, module)?)?;
    module.add(
        "FutureAccessError",
        module.py().get_type::<FutureAccessError>(),
    )?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
