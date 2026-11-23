use std::collections::BTreeMap;
use std::path::Path;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use sha2::{Digest, Sha256};

use crate::error::EngineError;
use crate::time::{format_date, parse_timestamp};

const CSV_HEADER: [&str; 9] = [
    "symbol",
    "ts",
    "published_at",
    "version",
    "open",
    "high",
    "low",
    "close",
    "volume",
];

/// Immutable point-in-time store.
///
/// Every row carries the timestamp of the observation and the timestamp at
/// which it became public. A read as of `T` only sees rows whose
/// `published_at <= T`, and for each bar timestamp it picks the newest such
/// version. Revisions are therefore invisible until their publication time.
#[pyclass(name = "PointInTimeStore")]
#[derive(Clone, Debug)]
pub struct PointInTimeStore {
    rows: Vec<BarRow>,
    by_symbol: BTreeMap<String, Vec<usize>>,
    /// Indices of the version-1 rows that define market events.
    event_rows: Vec<usize>,
    data_hash: String,
    symbols: Vec<String>,
    first_ts: Option<i64>,
    last_ts: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct BarRow {
    pub bar: crate::types::Bar,
    /// Set on the version-1 row of each (symbol, timestamp).
    pub is_initial: bool,
}

impl PointInTimeStore {
    pub fn from_rows(mut rows: Vec<crate::types::Bar>) -> Result<Self, EngineError> {
        if rows.is_empty() {
            return Err(EngineError::Data("dataset is empty".into()));
        }
        for (index, bar) in rows.iter().enumerate() {
            validate_bar(bar, index)?;
        }

        rows.sort_by(|a, b| {
            (a.ts, &a.symbol, a.published_at, a.version).cmp(&(
                b.ts,
                &b.symbol,
                b.published_at,
                b.version,
            ))
        });

        // Reject duplicate (symbol, ts, version) triples and make sure every
        // event timestamp has a version-1 row published exactly at the event.
        for window in rows.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            if a.symbol == b.symbol && a.ts == b.ts && a.version == b.version {
                return Err(EngineError::Data(format!(
                    "duplicate version {} for {} at {}",
                    a.version,
                    a.symbol,
                    format_date(a.ts)
                )));
            }
        }

        let mut event_rows = Vec::new();
        let mut index = 0;
        while index < rows.len() {
            let symbol = rows[index].symbol.clone();
            let ts = rows[index].ts;
            let mut end = index + 1;
            while end < rows.len() && rows[end].symbol == symbol && rows[end].ts == ts {
                end += 1;
            }
            let mut initial = None;
            for (offset, row) in rows[index..end].iter().enumerate() {
                if row.version == 1 {
                    initial = Some(index + offset);
                }
            }
            match initial {
                None => {
                    return Err(EngineError::Data(format!(
                        "missing version 1 for {symbol} at {}",
                        format_date(ts)
                    )));
                }
                Some(position) if rows[position].published_at != ts => {
                    return Err(EngineError::Data(format!(
                        "version 1 for {symbol} at {} must be published at its own timestamp",
                        format_date(ts)
                    )));
                }
                Some(position) => event_rows.push(position),
            }
            index = end;
        }

        let mut by_symbol: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, row) in rows.iter().enumerate() {
            by_symbol.entry(row.symbol.clone()).or_default().push(index);
        }
        let symbols: Vec<String> = by_symbol.keys().cloned().collect();
        let first_ts = event_rows.iter().map(|&i| rows[i].ts).min();
        let last_ts = event_rows.iter().map(|&i| rows[i].ts).max();

        let mut store = Self {
            rows: rows
                .into_iter()
                .map(|bar| BarRow {
                    bar,
                    is_initial: false,
                })
                .collect(),
            by_symbol,
            event_rows,
            data_hash: String::new(),
            symbols,
            first_ts,
            last_ts,
        };
        for &index in &store.event_rows {
            store.rows[index].is_initial = true;
        }
        store.data_hash = compute_data_hash(&store.rows);
        Ok(store)
    }

    pub fn data_hash(&self) -> &str {
        &self.data_hash
    }

    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn revision_count(&self) -> usize {
        self.rows.len() - self.event_rows.len()
    }

    pub fn first_ts(&self) -> Option<i64> {
        self.first_ts
    }

    pub fn last_ts(&self) -> Option<i64> {
        self.last_ts
    }

    /// Latest version visible at `as_of` for the most recent bar no later
    /// than `as_of`.
    pub fn as_of(&self, symbol: &str, as_of: i64) -> Option<crate::types::Bar> {
        let indices = self.by_symbol.get(symbol)?;
        let mut found = None;
        for &index in indices {
            let row = &self.rows[index].bar;
            if row.ts <= as_of && row.published_at <= as_of {
                found = Some(row);
            }
        }
        found.cloned()
    }

    /// The `limit` most recent completed bars strictly before `as_of`, each at
    /// its newest version published by `as_of`.
    pub fn history_before(&self, symbol: &str, as_of: i64, limit: usize) -> Vec<crate::types::Bar> {
        if limit == 0 {
            return Vec::new();
        }
        let Some(indices) = self.by_symbol.get(symbol) else {
            return Vec::new();
        };
        let mut history: Vec<crate::types::Bar> = Vec::new();
        for &index in indices {
            let row = &self.rows[index].bar;
            if row.ts < as_of && row.published_at <= as_of {
                if history.last().map(|bar| bar.ts) == Some(row.ts) {
                    *history.last_mut().expect("history is non-empty") = row.clone();
                } else {
                    history.push(row.clone());
                }
            }
        }
        if history.len() > limit {
            history.drain(0..history.len() - limit);
        }
        history
    }

    /// Every bar of a symbol at its newest version, without any visibility
    /// filter. This is the raw research view; the engine only exposes it
    /// through `Context::research_series`, which is audited and rejected in
    /// strict mode.
    pub fn latest_series(&self, symbol: &str) -> Vec<crate::types::Bar> {
        let Some(indices) = self.by_symbol.get(symbol) else {
            return Vec::new();
        };
        let mut series: Vec<crate::types::Bar> = Vec::new();
        for &index in indices {
            let row = &self.rows[index].bar;
            if series.last().map(|bar| bar.ts) == Some(row.ts) {
                *series.last_mut().expect("series is non-empty") = row.clone();
            } else {
                series.push(row.clone());
            }
        }
        series
    }

    /// Every bar visible at `as_of` (including the current bar), each at its
    /// newest published version.
    pub fn series_as_of(&self, symbol: &str, as_of: i64) -> Vec<crate::types::Bar> {
        let Some(indices) = self.by_symbol.get(symbol) else {
            return Vec::new();
        };
        let mut series: Vec<crate::types::Bar> = Vec::new();
        for &index in indices {
            let row = &self.rows[index].bar;
            if row.ts <= as_of && row.published_at <= as_of {
                if series.last().map(|bar| bar.ts) == Some(row.ts) {
                    *series.last_mut().expect("series is non-empty") = row.clone();
                } else {
                    series.push(row.clone());
                }
            }
        }
        series
    }

    /// Version-1 bars that become market events, in deterministic
    /// `(timestamp, symbol)` order.
    pub fn events_in_range(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Vec<crate::types::Bar> {
        self.event_rows
            .iter()
            .map(|&index| self.rows[index].bar.clone())
            .filter(|bar| start_ts.is_none_or(|start| bar.ts >= start))
            .filter(|bar| end_ts.is_none_or(|end| bar.ts < end))
            .collect()
    }

    /// Unique event timestamps in ascending order.
    pub fn event_timestamps(&self) -> Vec<i64> {
        let mut out = Vec::new();
        for &index in &self.event_rows {
            let ts = self.rows[index].bar.ts;
            if out.last() != Some(&ts) {
                out.push(ts);
            }
        }
        out
    }

    pub fn all_rows(&self) -> Vec<crate::types::Bar> {
        self.rows.iter().map(|row| row.bar.clone()).collect()
    }
}

#[pymethods]
impl PointInTimeStore {
    #[staticmethod]
    fn from_csv(path: &str) -> PyResult<Self> {
        let text = std::fs::read_to_string(Path::new(path))
            .map_err(|error| PyRuntimeError::new_err(format!("cannot read {path:?}: {error}")))?;
        Self::from_csv_str(&text)
    }

    #[staticmethod]
    fn from_csv_str(text: &str) -> PyResult<Self> {
        let rows = parse_csv(text).map_err(PyErr::from)?;
        Self::from_rows(rows).map_err(PyErr::from)
    }

    #[getter]
    fn get_data_hash(&self) -> String {
        self.data_hash.clone()
    }

    fn len(&self) -> usize {
        self.row_count()
    }

    fn __len__(&self) -> usize {
        self.row_count()
    }

    #[getter]
    fn get_bar_count(&self) -> usize {
        self.event_rows.len()
    }

    #[getter]
    fn get_revision_count(&self) -> usize {
        self.revision_count()
    }

    #[getter]
    fn get_first_ts(&self) -> Option<i64> {
        self.first_ts()
    }

    #[getter]
    fn get_last_ts(&self) -> Option<i64> {
        self.last_ts()
    }

    #[getter]
    fn first_date(&self) -> Option<String> {
        self.first_ts.map(format_date)
    }

    #[getter]
    fn last_date(&self) -> Option<String> {
        self.last_ts.map(format_date)
    }

    #[pyo3(name = "symbols")]
    fn py_symbols(&self) -> Vec<String> {
        self.symbols.to_vec()
    }

    fn timestamps(&self) -> Vec<i64> {
        self.event_timestamps()
    }

    fn rows(&self) -> Vec<crate::types::Bar> {
        self.all_rows()
    }

    #[pyo3(name = "as_of")]
    fn py_as_of(&self, symbol: &str, ts: i64) -> Option<crate::types::Bar> {
        self.as_of(symbol, ts)
    }

    #[pyo3(name = "history_before")]
    fn py_history_before(&self, symbol: &str, as_of: i64, limit: usize) -> Vec<crate::types::Bar> {
        self.history_before(symbol, as_of, limit)
    }

    fn __repr__(&self) -> String {
        format!(
            "PointInTimeStore(rows={}, bars={}, revisions={}, symbols={:?})",
            self.row_count(),
            self.event_rows.len(),
            self.revision_count(),
            self.symbols
        )
    }
}

fn validate_bar(bar: &crate::types::Bar, index: usize) -> Result<(), EngineError> {
    let label = format!(
        "row {} ({} at {})",
        index + 1,
        bar.symbol,
        format_date(bar.ts)
    );
    if bar.symbol.trim().is_empty() {
        return Err(EngineError::Data(format!("{label}: empty symbol")));
    }
    if bar.version == 0 {
        return Err(EngineError::Data(format!("{label}: version must be >= 1")));
    }
    if bar.published_at < bar.ts {
        return Err(EngineError::Data(format!(
            "{label}: published_at precedes the bar timestamp"
        )));
    }
    for (name, value) in [
        ("open", bar.open),
        ("high", bar.high),
        ("low", bar.low),
        ("close", bar.close),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(EngineError::Data(format!(
                "{label}: {name} must be finite and positive"
            )));
        }
    }
    if !bar.volume.is_finite() || bar.volume <= 0.0 {
        return Err(EngineError::Data(format!(
            "{label}: volume must be finite and positive"
        )));
    }
    if bar.high < bar.low
        || bar.high < bar.open
        || bar.high < bar.close
        || bar.low > bar.open
        || bar.low > bar.close
    {
        return Err(EngineError::Data(format!(
            "{label}: inconsistent OHLC values"
        )));
    }
    Ok(())
}

fn parse_csv(text: &str) -> Result<Vec<crate::types::Bar>, EngineError> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|error| EngineError::Data(format!("cannot read CSV header: {error}")))?;
    if headers.len() != CSV_HEADER.len()
        || headers
            .iter()
            .zip(CSV_HEADER)
            .any(|(actual, expected)| actual != expected)
    {
        return Err(EngineError::Data(format!(
            "CSV header must be exactly: {}",
            CSV_HEADER.join(",")
        )));
    }

    let mut bars = Vec::new();
    for (line, record) in reader.records().enumerate() {
        let record =
            record.map_err(|error| EngineError::Data(format!("CSV line {}: {error}", line + 2)))?;
        if record.len() != CSV_HEADER.len() {
            return Err(EngineError::Data(format!(
                "CSV line {}: expected {} columns, found {}",
                line + 2,
                CSV_HEADER.len(),
                record.len()
            )));
        }
        let parse_f64 = |name: &str, value: &str| -> Result<f64, EngineError> {
            value.parse::<f64>().map_err(|_| {
                EngineError::Data(format!("CSV line {}: invalid {name} {value:?}", line + 2))
            })
        };
        let ts = parse_timestamp(&record[1])
            .map_err(|error| EngineError::Data(format!("CSV line {}: {error}", line + 2)))?;
        let published_at = parse_timestamp(&record[2])
            .map_err(|error| EngineError::Data(format!("CSV line {}: {error}", line + 2)))?;
        let version = record[3].parse::<u32>().map_err(|_| {
            EngineError::Data(format!(
                "CSV line {}: invalid version {:?}",
                line + 2,
                &record[3]
            ))
        })?;
        bars.push(crate::types::Bar {
            symbol: record[0].to_string(),
            ts,
            published_at,
            version,
            open: parse_f64("open", &record[4])?,
            high: parse_f64("high", &record[5])?,
            low: parse_f64("low", &record[6])?,
            close: parse_f64("close", &record[7])?,
            volume: parse_f64("volume", &record[8])?,
        });
    }
    Ok(bars)
}

fn compute_data_hash(rows: &[BarRow]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"backtest-engine/pit/v1\0");
    for row in rows {
        let bar = &row.bar;
        hasher.update(bar.symbol.as_bytes());
        hasher.update([0]);
        hasher.update(bar.ts.to_le_bytes());
        hasher.update(bar.published_at.to_le_bytes());
        hasher.update(bar.version.to_le_bytes());
        hasher.update(bar.open.to_bits().to_le_bytes());
        hasher.update(bar.high.to_bits().to_le_bytes());
        hasher.update(bar.low.to_bits().to_le_bytes());
        hasher.update(bar.close.to_bits().to_le_bytes());
        hasher.update(bar.volume.to_bits().to_le_bytes());
        hasher.update(if row.is_initial { [1u8] } else { [0u8] });
    }
    to_hex(&hasher.finalize())
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Bar;

    fn bar(ts: i64, published_at: i64, version: u32, close: f64) -> Bar {
        Bar {
            symbol: "ACME".into(),
            ts,
            published_at,
            version,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1_000.0,
        }
    }

    #[test]
    fn revision_is_invisible_until_publication() {
        let store = PointInTimeStore::from_rows(vec![
            bar(10, 10, 1, 100.0),
            bar(10, 30, 2, 200.0),
            bar(20, 20, 1, 110.0),
        ])
        .unwrap();
        assert_eq!(store.as_of("ACME", 15).unwrap().close, 100.0);
        assert_eq!(store.as_of("ACME", 20).unwrap().close, 110.0);
        // At t=20 the most recent visible bar is still the version-1 row.
        assert_eq!(store.as_of("ACME", 25).unwrap().close, 110.0);
        assert_eq!(store.as_of("ACME", 30).unwrap().close, 110.0);
        // The revision only becomes visible together with the later bar.
        assert_eq!(store.as_of("ACME", 35).unwrap().close, 110.0);
        assert_eq!(store.history_before("ACME", 35, 10).len(), 2);
        assert_eq!(store.history_before("ACME", 35, 10)[1].close, 110.0);
    }

    #[test]
    fn revision_replaces_old_value_after_publication() {
        let store = PointInTimeStore::from_rows(vec![
            bar(10, 10, 1, 100.0),
            bar(10, 15, 2, 101.0),
            bar(20, 20, 1, 110.0),
        ])
        .unwrap();
        assert_eq!(store.history_before("ACME", 14, 10)[0].close, 100.0);
        assert_eq!(store.history_before("ACME", 16, 10)[0].close, 101.0);
    }

    #[test]
    fn data_hash_is_independent_of_row_order() {
        let rows = vec![
            bar(10, 10, 1, 100.0),
            bar(20, 20, 1, 110.0),
            bar(10, 25, 2, 102.0),
        ];
        let mut shuffled = rows.clone();
        shuffled.reverse();
        let a = PointInTimeStore::from_rows(rows).unwrap();
        let b = PointInTimeStore::from_rows(shuffled).unwrap();
        assert_eq!(a.data_hash(), b.data_hash());
        assert_eq!(a.events_in_range(None, None).len(), 2);
        assert_eq!(a.revision_count(), 1);
    }

    #[test]
    fn rejects_missing_initial_version() {
        let error = PointInTimeStore::from_rows(vec![bar(10, 20, 2, 100.0)]).unwrap_err();
        assert!(error.to_string().contains("missing version 1"));
    }

    #[test]
    fn rejects_publication_before_bar() {
        let error = PointInTimeStore::from_rows(vec![bar(10, 9, 1, 100.0)]).unwrap_err();
        assert!(error.to_string().contains("published_at precedes"));
    }
}
