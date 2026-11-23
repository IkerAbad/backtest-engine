# backtest-engine

[![CI](https://github.com/IkerAbad/backtest-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/IkerAbad/backtest-engine/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/backtest-engine.svg)](https://pypi.org/project/backtest-engine/)

Event-driven backtesting engine that treats look-ahead bias as something to **prove absent**, not
something to avoid by convention. The core is Rust, strategies are Python, and every reported result
is reproduced against a future-masked replay before an HTML report is written.

- Deterministic event queue ordered by `(data timestamp, symbol)`, with a simulated clock.
- Point-in-time store with row versioning: a revision published at `T+2d` is invisible at `T`.
- Strategies receive one bar at a time; the default data view is bounded by the clock.
- A detector runs the same strategy twice, once with the future masked, and fails on any difference.
- Configurable commission, spread and volume-proportional slippage.
- Walk-forward folds with non-overlapping train and test windows.
- Real-data case study: 10 years of SPY, walk-forward, buy-and-hold comparison and a subtle
  full-sample look-ahead strategy that the detector catches.
- Multi-asset case study: SPY and TLT, relative momentum, per-asset costs and a cross-sectional
  future trap that the detector catches.
- Self-contained HTML reports whose SHA-256 is verified on Linux, Windows and macOS in CI.

## Why Rust with PyO3

The engine (queue, clock, point-in-time store, portfolio, costs, metrics, report) is Rust; strategies
are ordinary Python objects. PyO3 and maturin package the extension, and the build targets the
stable ABI (`abi3-py310`), so one wheel works on Python 3.10+.

Reasons for this choice:

1. **The correctness-critical part is compiled and typed.** Event ordering, version selection,
   cash/position accounting and metric accumulation are in one place, checked by `cargo test` and
   clippy. Python only expresses the strategy, which is the part that benefits from a dynamic
   language.
2. **No garbage collector in the simulation loop.** The clock and portfolio state do not depend on
   Python object lifetime or finalizer order.
3. **Deterministic floating-point behaviour.** The core avoids `HashMap` iteration, wall-clock
   metadata, locale formatting and transcendental functions. The Rust compiler does not enable
   fast-math or FP contraction, so the same operations produce the same bits on the supported
   platforms.
4. **Good packaging story.** `maturin` + `pyproject.toml` gives a normal `pip install .` and an
   importable package with `.pyi` stubs; no hand-written C++ build system.
5. **Safer than pybind11 for this code size.** pybind11 is mature and was a valid option, but the
   store and engine move `String`, `Vec` and shared state across the boundary. Rust's ownership
   model and clippy catch classes of mistakes that would be manual review work in C++.

The trade-off is a Rust toolchain at build time for source installs. Prebuilt `abi3` wheels are
published on PyPI, so `pip install backtest-engine` needs no compiler.

## Install

```bash
python -m pip install backtest-engine
```

From a checkout, for development:

```bash
python -m venv .venv
# Windows: .venv\Scripts\activate
# Linux/macOS: source .venv/bin/activate
python -m pip install maturin
maturin develop --release
python examples/run_example.py
```

Or build and install the wheel path used by CI:

```bash
python -m pip install .
```

Commands:

```bash
# Full backtest with costs, look-ahead check, and HTML report
python -m backtest_engine backtest --strategy moving_average --window 20 \
    --report reports/example.html

# Run only the detector
python -m backtest_engine check --strategy moving_average

# Deliberate cheat: exits 0 only because --expect-detected confirms the rejection
python -m backtest_engine check --strategy lookahead_trap --expect-detected

# Subtle cheat: full-sample normalization, also caught
python -m backtest_engine check --strategy full_sample_zscore --expect-detected

# Walk-forward with non-overlapping test windows
python -m backtest_engine walkforward --strategy moving_average \
    --windows 5,10,20,50 --train 252 --test 63
```

`backtest` refuses to write a performance report if the look-ahead check fails. `--no-check` exists
for debugging only and marks the report verdict accordingly.

## Strategy interface

A strategy is a Python object with `on_bar(ctx, bar) -> list[Order]`. `on_start(ctx)` and
`on_finish(ctx)` are optional. State must live on the instance.

```python
from backtest_engine import Bar, Context, Order


class Breakout:
    def __init__(self, window: int = 20) -> None:
        self.window = window
        self._closes: list[float] = []

    def on_bar(self, ctx: Context, bar: Bar) -> list[Order]:
        self._closes.append(bar.close)
        if len(self._closes) > self.window:
            self._closes.pop(0)
        if len(self._closes) < self.window:
            return []
        high = max(self._closes)
        return [ctx.order(bar.symbol, 1.0 if bar.close >= high else 0.0)]
```

What `ctx` exposes:

| Member | Meaning |
|---|---|
| `ctx.now`, `ctx.date` | Simulated clock (Unix seconds / ISO date) |
| `ctx.equity`, `ctx.cash` | Mark-to-market portfolio state |
| `ctx.position(symbol)` | Current signed quantity |
| `ctx.price(symbol)` | Latest close visible at the clock |
| `ctx.history(symbol, n)` | Last `n` completed bars, point-in-time, strictly before the clock |
| `ctx.as_of(symbol, ts)` | Latest version visible at `ts`; future requests are rejected by default |
| `ctx.research_series(symbol)` | Audited escape hatch: the whole stored series, including the future. Rejected in strict mode; masked in the detector |
| `ctx.order(symbol, weight)` | Target weight in `[-1, 1]`; executes at the next bar's open |

Orders are target weights, not market orders. The engine sizes the quantity from current equity and
schedules the fill for the next event of that symbol. A signal on bar `T` can never fill on bar `T`.

`ctx.research_series` exists only to model the common mistake of handing a complete DataFrame to a
strategy. It is refused by default (`strict_future_access=True`) and truncated at the clock in the
detector's masked replay. The engine cannot inspect data that a strategy loads outside its context;
if you precompute indicators in a separate pandas object, the detector has nothing to instrument.
Use the engine's data view for anything you intend to publish.

## Point-in-time data

CSV columns, in this exact order:

```text
symbol,ts,published_at,version,open,high,low,close,volume
```

- `ts`: timestamp of the observation.
- `published_at`: timestamp at which this version became public.
- `version`: `1` for the initial row, `2+` for revisions.
- The version-1 row must be published exactly at `ts`; revisions must be published later.
- `as_of(T)` considers only rows with `ts <= T` and `published_at <= T`, then keeps the newest
  version of the newest bar.
- `history(n)` returns completed bars strictly before the clock, each at the newest version already
  published.

The committed sample (`data/sample.csv`) has 780 daily bars of `ACME` from 2021-01-04 to 2023-12-29
and 12 restatements published two or thirty days later. Data SHA-256 is printed in the report and
included in the report bytes.

The sample is synthetic and generated by `scripts/generate_data.py` with a fixed-seed LCG. It is
committed, so CI never regenerates it.

## Look-ahead detector

`check_lookahead(factory, store, config)` runs two fresh instances of the same strategy:

1. **Normal run.** `strict_future_access=False`, every `as_of` request is allowed and logged.
2. **Masked run.** Every request after the simulated clock returns `None`.

It then compares every decision, every fill count and final equity. The check fails if:

- a future read was logged,
- any decision (timestamp, symbol, target weights) differs,
- executions differ even though no future read was logged (for example, a strategy with global
  mutable state or an unseeded random generator).

The report names the first divergent decision and the first future read:

```text
look-ahead check: FAILED
strategy: Look-ahead trap
future reads: 780
first future read: 2021-01-04 asked for 2021-01-05 (ACME)
first divergent decision: #0 at 2021-01-04 (ACME)
  normal: ACME +1.000000
  masked: ACME +0.000000
divergent decisions: 780
execution diverged: normal equity=26132170.937502 masked equity=1000000.000000
```

The `LookAheadTrap` implementation reads the next bar's close with
`ctx.as_of(bar.symbol, ctx.now + 86400)`. In the normal run it goes long before up days and short
before down days; in the masked run it sees nothing and stays flat. The detector catches it at the
first bar, and the pytest suite asserts that exact failure (`tests/test_detector.py`). CI also runs
`check --strategy lookahead_trap --expect-detected`.

With the default config, a raw `run_backtest` refuses future reads immediately with
`FutureAccessError`; the detector opts into the audited/masked modes explicitly.

A second example, `FullSampleZScore`, uses `ctx.research_series` to compute the mean and standard
deviation of the whole sample and normalizes each close with them. It never reads a future price
directly. The detector still fails it: the normal run logs one `research_series` read through the
end of the dataset, the masked run only sees bars up to the clock, and the first target weight
changes from `+0.660893` to `+0.000000` on the real SPY data. `check --strategy
full_sample_zscore --expect-detected` returns 0 only because the rejection is confirmed.

## Cost model

All parameters are on `EngineConfig`:

| Parameter | Default | Applied as |
|---|---:|---|
| `commission_bps` | 1.0 | `notional * bps / 10_000` |
| `commission_fixed` | 0.0 | fixed cash per fill |
| `spread_bps` | 2.0 | half the spread on each side, embedded in the fill price |
| `slippage_bps_at_full_volume` | 100.0 | `bps * (quantity / bar_volume)`, linear in participation |

For a buy, the fill price is `open * (1 + (spread_bps/2 + slippage_bps)/10_000)`; a sell uses the
same expression with a negative sign. Commission, spread and slippage are tracked separately and
reported in cash and in basis points of initial capital. Execution is at the **next bar's open**,
with that bar's volume used for impact. Fractional shares are allowed.

## Example results

`data/sample.csv`, moving average window 20, initial capital 1,000,000, 1 bp commission, 2 bps
spread, 100 bps slippage at full participation, 252 periods per year:

| Metric | With costs | Without costs |
|---|---:|---:|
| Total return | 78.7343% | 82.2168% |
| Annualized volatility | 12.4574% | 12.4658% |
| Sharpe ratio | 1.5707 | 1.6199 |
| Max drawdown | -10.5556% | -10.0922% |
| Turnover | 55.2839x | 55.3158x |
| Fills | 168 | 164 |
| Total cost | 26,580.07 (265.80 bps) | 0.00 (0.00 bps) |
| Final equity | 1,787,342.85 | 1,822,168.34 |

Costs reduce the total return by 3.48 percentage points and the Sharpe ratio by 0.049. The
without-cost run is the same engine and data with all frictions set to zero
(`EngineConfig.without_costs()`), not a separate implementation.

The full-period result is in-sample for the chosen window. The walk-forward command selects the
window on 252-bar training blocks and reports only the following 63-bar blocks; on this dataset its
aggregate out-of-sample return is 16.3799% with a Sharpe of 0.7733 over 8 folds. That gap is the
point of walk-forward.

## Real-data case studies

### Single asset: SPY

The committed report is [`docs/case_study.html`](docs/case_study.html) and is also served at
[ikerabad.github.io/backtest-engine/case_study.html](https://ikerabad.github.io/backtest-engine/case_study.html).
It is generated only from committed inputs: `data/spy_daily.csv`, its metadata and the engine.

- Asset: SPY, 2,514 daily bars, 2016-11-21 to 2026-11-20.
- Source: Yahoo Finance chart API, OHLC adjusted with Yahoo's `adjclose`, volume unchanged.
- Data hash: CSV SHA-256 `2db20ca1bd97775f20b913b2b4fc9620c3bbf130b735a1e1d1b6e0b4f3da8fff`,
  engine canonical hash `4f5c488080735c1d7fd06b4e206ef8dda958f5c9671df3f99791a558a7cd6ebb`.
- Walk-forward: 504-bar training, 252-bar test, 252-bar step, windows 20/50/100/200, 7 folds.
- Costs: the same 1 bp commission, 2 bps spread and 100 bps full-volume slippage.

| Out-of-sample metric | Walk-forward strategy | Buy and hold |
|---|---:|---:|
| Total return | 69.1985% | 187.5521% |
| Annualized volatility | 10.0943% | 20.0574% |
| Sharpe ratio | 0.7980 | 0.8560 |
| Max drawdown | -12.9362% | -33.7173% |

The strategy **underperforms buy-and-hold** on total return, and its Sharpe is slightly lower. It
does cut volatility and the maximum drawdown. The walk-forward run made 257 fills. The report
publishes that result; nothing is hand-entered.

### Subtle full-sample look-ahead

`FullSampleZScore` is the second cheat. It computes the mean and standard deviation of the **entire
sample** and uses them to normalize every close. It does not read tomorrow's price and it does not
trade on a future bar directly, so the leak is purely statistical.

The detector catches it on the real data:

| Field | Value |
|---|---|
| Check | FAILED |
| Future reads | 1, kind `research_series` |
| First read | 2016-11-21, requesting through 2026-11-20 |
| First divergent decision | #0 on 2016-11-21, SPY |
| Normal target weight | +0.660893 |
| Masked target weight | +0.000000 |
| Divergent decisions | 2,514 |
| Normal final equity | 1,209,445.25 |
| Masked final equity | 1,000,000.00 |

The masked replay sees only bars up to the clock, so the first standard deviation is zero and the
strategy stays flat. The detector reports the first decision where the traces differ. Strict mode
rejects `research_series` before a raw report can be produced, and the pytest suite asserts the
failure on both the synthetic and the real dataset.

### Multi-asset: SPY and TLT

The two-asset report is [`docs/multi_asset.html`](docs/multi_asset.html) and is served at
[ikerabad.github.io/backtest-engine/multi_asset.html](https://ikerabad.github.io/backtest-engine/multi_asset.html).
It uses `data/spy_tlt_daily.csv`: 5,028 rows, 2,514 timestamps, two symbols, same 2016-2026 range.

The strategy is `RelativeMomentum`: on the first bar of each month it computes each asset's return
over a lookback window and puts 100% of equity in the winner. Walk-forward uses 504-bar training,
252-bar test, 252-bar step and a lookback grid of 63/126/252 over 7 folds.

| Out-of-sample metric | Relative momentum | Static 50/50 |
|---|---:|---:|
| Total return | 34.2857% | 91.2796% |
| Annualized volatility | 17.8669% | 12.2823% |
| Sharpe ratio | 0.3258 | 0.8189 |
| Max drawdown | -46.6757% | -27.8624% |

The strategy **underperforms the static 50/50 reference** and has a much deeper drawdown. It made 93
fills and selected SPY in 69 rebalances and TLT in 22. Total costs were 8,182.70. The result is
published as measured.

Capital allocation and per-asset costs:

- Target weights are fractions of total equity. This strategy uses `w_SPY + w_TLT = 1`, so gross
  exposure is 100% and cash is only the residual after fills. The engine does not enforce a budget;
  the strategy is responsible for the sum of absolute weights.
- Each asset's fill is charged on its own notional and volume:
  `commission_i = fixed + bps * |qty_i| * open_i / 10_000`. A switch from SPY to TLT produces two
  fills, so the fixed commission is paid twice. Spread and slippage use that asset's own bar.

| Asset | Fills | Notional | Commission | Spread | Slippage | Total |
|---|---:|---:|---:|---:|---:|---:|
| SPY | 62 | 22,633,022.92 | 2,263.30 | 2,263.30 | 9.00 | 4,535.61 |
| TLT | 31 | 17,556,560.78 | 1,755.66 | 1,755.66 | 135.78 | 3,647.09 |
| Total | 93 | 40,189,583.70 | 4,018.96 | 4,018.96 | 144.78 | 8,182.70 |

The multi-asset detector cases:

- The honest `RelativeMomentum` replay passes with 0 future reads and 0 divergent decisions.
- `MultiAssetFutureTrap` peeks at both assets' next bar on every rebalance. The detector logs 242
  future reads and 121 divergent decisions. The first is decision #0 on 2016-11-21: normal
  `SPY +1.000000, TLT +0.000000`; masked `SPY +0.000000, TLT +0.000000`. Normal final equity is
  2,025,864.26 against 1,000,000.00 masked.
- `tests/test_multi_asset.py` asserts the detection, the allocation split and the per-asset
  commissions. Strict mode rejects the trap before a raw report can be produced.

## Measured performance

One 10-year daily backtest, release build, measured on 2026-11-23:

| Field | Value |
|---|---|
| Hardware | AMD Ryzen 9 5900X, 32 GB DDR4-3600 CL16 |
| OS / Python / Rust | Windows 11 Pro 10.0.26300 / CPython 3.12.10 / 1.100.0 |
| Dataset | SPY daily, 2,514 bars |
| Strategy | `MovingAverage(window=100)` |
| Median time | 11.01 ms per backtest |
| Fastest / slowest | 10.72 ms / 12.00 ms |
| Throughput | 228,387 events/second |
| Peak process RSS | 25.17 MB |

Timing wraps only `run_backtest`; CSV parsing and interpreter startup are excluded. A run includes
the event queue, point-in-time reads, 2,514 Python callbacks, order sizing, fills, accounting and
metrics. Peak RSS includes the interpreter and the loaded dataset. The full method and raw JSON are
in [`docs/benchmark.md`](docs/benchmark.md) and [`docs/benchmark.json`](docs/benchmark.json).
These numbers are machine-specific and are not part of the report hash.

## Determinism

The same data and strategy produce the same report bytes, and CI verifies the committed SHA-256 on
Ubuntu, Windows and macOS:

- Event ordering is `(ts, symbol)`; insertion order and CSV row order are irrelevant.
- All report-relevant collections are `BTreeMap`/`Vec`; no hash iteration reaches the report.
- The report contains no wall-clock timestamp, host name, path or locale-dependent number.
- Floats are formatted by Rust with fixed precision, and the HTML is written as raw UTF-8 bytes so
  Windows cannot rewrite line endings.
- The reference strategies use only basic IEEE-754 operations and `math.sqrt`; no NumPy, no random
  module, no `pow`.
- The committed engine report hash lives in `reports/expected.sha256` and is checked by
  `scripts/verify_report.py` on Ubuntu, Windows and macOS.
- The single-asset case study hash lives in `docs/case_study.sha256` and is checked by
  `scripts/verify_case_study.py` on all three platforms.
- The multi-asset case study hash lives in `docs/multi_asset.sha256` and is checked by
  `scripts/verify_multi_asset_case.py` on all three platforms.

`tests/test_report.py` also checks that two in-process runs produce byte-identical HTML.

## Walk-forward

`walk_forward` builds folds from the store's event timestamps. For a fold, the training window is
`[start, start + train_bars)` and the test window starts at `start + train_bars`. `step_bars`
defaults to `test_bars` and may not be smaller, so test windows are contiguous and never overlap.
Every parameter set is scored on the training window only (Sharpe, then total return, then grid
order). The winner is run once on the test window.

```python
from backtest_engine import EngineConfig, MovingAverage, PointInTimeStore, walk_forward


def factory(**params: object) -> MovingAverage:
    return MovingAverage(window=int(str(params["window"])))


store = PointInTimeStore.from_csv("data/sample.csv")
result = walk_forward(
    factory,
    store,
    EngineConfig(),
    train_bars=252,
    test_bars=63,
    grid=[{"window": 5}, {"window": 10}, {"window": 20}, {"window": 50}],
)
print(result.total_return, result.sharpe, result.max_drawdown)
```

## Project layout

```text
src/
  engine.rs     queue loop, clock, portfolio, order sizing, fills
  store.rs      point-in-time store, CSV parser, versioning, data hash
  queue.rs      min-heap ordered by (timestamp, symbol)
  costs.rs      commission / spread / volume-proportional slippage
  metrics.rs    return, volatility, Sharpe, drawdown, turnover, costs
  report.rs     deterministic self-contained HTML + inline SVG
  types.rs      Bar, Order, Trade, Decision, DataAccess, EngineConfig, result
  lib.rs        PyO3 module and Python strategy adapter
python/backtest_engine/
  strategies.py moving average, mean reversion, look-ahead trap, full-sample z-score,
                relative momentum and the multi-asset future trap
  detector.py   normal vs masked replay and divergence report
  walkforward.py non-overlapping folds and aggregate out-of-sample metrics
  report.py     byte-exact report writer
  cli.py        backtest / check / walkforward commands
  _backtest_engine.pyi  complete type stubs
data/sample.csv committed synthetic point-in-time dataset
data/spy_daily.csv committed real SPY history and metadata
data/spy_tlt_daily.csv committed real SPY/TLT history and metadata
docs/            case studies, index page and benchmark results
reports/         committed engine report and expected SHA-256
scripts/         data fetch, case study builders, verifiers and benchmark
tests/           pytest acceptance, real-data, multi-asset and determinism tests
```

The Rust core also has unit tests for queue ordering, revision visibility, cost arithmetic, metric
math, next-open execution and strict/masked modes (`cargo test`).

## CI

`.github/workflows/ci.yml` runs:

1. **Rust core**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
   `cargo test --locked`.
2. **Python**: on Ubuntu, Windows and macOS, build with maturin, run `pytest`, `ruff check`,
   `ruff format --check` and `mypy --strict`.
3. **Reproducible engine report**: on all three OSes, run `examples/run_example.py`, verify
   `reports/expected.sha256`, and confirm that the deliberate trap is detected.
4. **Real-data case studies**: on all three OSes, rebuild the single-asset and multi-asset reports,
   verify `docs/case_study.sha256` and `docs/multi_asset.sha256`, and confirm that both the subtle
   full-sample strategy and the multi-asset future trap are detected. The reports are uploaded as
   artifacts.

## Scope and limitations

In scope: event-driven simulation, point-in-time data, costs, look-ahead detection, walk-forward,
HTML reporting, deterministic hashing.

Out of scope: broker connections, paid data, portfolio optimisation, margin/borrow costs, dividends,
corporate actions, taxes, intraday calendars and survivorship-bias-free universe construction. The
synthetic sample has one symbol; the real-data multi-asset case covers two. The engine supports
arbitrary symbols and the portfolio accounting is per asset. Slippage is linear in participation, not
a square-root impact model. The real-data CSVs are snapshots from Yahoo Finance and are used for
research; check the provider's terms before commercial redistribution. A strategy that loads its own
DataFrame and precomputes full-sample indicators outside `ctx` is outside the detector's reach. This
is research software, not investment advice.

## License

MIT. Copyright (c) 2026 Iker Abad. See [LICENSE](LICENSE).
