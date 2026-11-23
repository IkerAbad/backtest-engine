# Measured performance

One daily backtest of 10 years of real SPY data. The numbers are machine-specific and are not
part of the reproducible report hash.

| Field | Value |
|---|---|
| Date | 2026-11-23 |
| Hardware | AMD Ryzen 9 5900X (12 cores / 24 threads) |
| Memory | 32 GB DDR4-3600 CL16 |
| OS | Windows 11 Pro, build 10.0.26300 |
| Python | CPython 3.12.10 |
| Rust | 1.100.0, release build |
| Dataset | `data/spy_daily.csv`, 2,514 bars |
| Period | 2016-11-21 to 2026-11-20 |
| Strategy | `MovingAverage(window=100)` |
| Costs | 1 bp commission, 2 bps spread, 100 bps slippage at full participation |
| Iterations | 5 warm-up + 50 measured runs |
| Median wall time | 11.01 ms |
| Fastest run | 10.72 ms |
| Slowest run | 12.00 ms |
| Throughput | 228,387 events/second |
| Peak process RSS | 25.17 MB |

## Method

The timer wraps only `run_backtest`, so CSV parsing and interpreter startup are excluded. A run
includes the event queue, point-in-time reads, 2,514 Python strategy callbacks, order sizing, fills,
mark-to-market accounting, metrics and result construction. Throughput is `bars / median seconds`.

Peak memory is the process peak working set reported by `GetProcessMemoryInfo`
(`PeakWorkingSetSize`) after the measured runs. It includes the CPython interpreter, the installed
extension and the loaded 2,514-row dataset. On Linux the script reads `VmHWM`; on macOS it reads
`ru_maxrss`.

The raw JSON is committed in [`benchmark.json`](benchmark.json) and can be regenerated with:

```bash
python scripts/benchmark_real_data.py --out docs/benchmark.json
```

Results vary with CPU frequency, background load and Python version; treat this as a single
machine's measurement, not a portable guarantee.
