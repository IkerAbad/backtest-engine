use std::fmt;

use crate::time::{format_date, format_timestamp};

/// Errors produced by the engine core.
#[derive(Debug, Clone)]
pub enum EngineError {
    Data(String),
    Config(String),
    Strategy(String),
    FutureAccess {
        symbol: String,
        clock: i64,
        requested: i64,
    },
}

impl EngineError {
    pub fn message(&self) -> String {
        match self {
            EngineError::Data(m) => format!("data error: {m}"),
            EngineError::Config(m) => format!("configuration error: {m}"),
            EngineError::Strategy(m) => format!("strategy error: {m}"),
            EngineError::FutureAccess {
                symbol,
                clock,
                requested,
            } => format!(
                "look-ahead denied: strategy asked for {symbol} as of {} while the simulated clock is {} ({})",
                format_timestamp(*requested),
                format_timestamp(*clock),
                format_date(*clock)
            ),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for EngineError {}
