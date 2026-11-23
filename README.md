# backtest-engine

[![CI](https://github.com/IkerAbad/backtest-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/IkerAbad/backtest-engine/actions/workflows/ci.yml)

Event-driven backtesting engine that treats look-ahead bias as something to **prove absent**, not
something to avoid by convention. The core is Rust, strategies are Python, and every reported result
is reproduced against a future-masked replay before an HTML report is written.

- Deterministic event queue ordered by `(data timestamp, symbol)`, with a simulated clock.
- Point-in-time store with row versioning: a revision published at `T+2d` is invisible at `T`.
- Strategies receive one bar at a time and can only touch a clock-bounded data view.
- A detector runs the same strategy twice, once with the future masked, and fails on any difference.
- Configurable commission, spread and volume-proportional slippage.
- Walk-forward folds with non-overlapping train and test windows.
- Self-contained HTML report whose SHA-256 is verified on Linux, Windows and macOS in CI.

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

The trade-off is a Rust toolchain at build time. Prebuilt wheels are not published; the source
build is one `pip install .`.

## Quickstart

```bash
python -m venv .venv
# Windows: .venv\Scripts\activate
# Linux/macOS: source .venv/bin/activate
python -m pip install maturin
maturin develop --release

python examples/run_example.py
```

Or install like a normal Python package (it builds the Rust extension):

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
| `ctx.order(symbol, weight)` | Target weight in `[-1, 1]`; executes at the next bar's open |

Orders are target weights, not market orders. The engine sizes the quantity from current equity and
schedules the fill for the next event of that symbol. A signal on bar `T` can never fill on bar `T`.

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
- The committed hash lives in `reports/expected.sha256`; `scripts/verify_report.py` recomputes it.

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
  strategies.py moving average, mean reversion, deliberate look-ahead trap
  detector.py   normal vs masked replay and divergence report
  walkforward.py non-overlapping folds and aggregate out-of-sample metrics
  report.py     byte-exact report writer
  cli.py        backtest / check / walkforward commands
  _backtest_engine.pyi  complete type stubs
data/sample.csv committed synthetic point-in-time dataset
reports/         committed example report and expected SHA-256
tests/           pytest acceptance and determinism tests
```

The Rust core also has unit tests for queue ordering, revision visibility, cost arithmetic, metric
math, next-open execution and strict/masked modes (`cargo test`).

## CI

`.github/workflows/ci.yml` runs:

1. **Rust core**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
   `cargo test --locked`.
2. **Python**: on Ubuntu, Windows and macOS, build with maturin, run `pytest`, `ruff check`,
   `ruff format --check` and `mypy --strict`.
3. **Reproducible report**: on all three OSes, run `examples/run_example.py`, verify
   `reports/expected.sha256`, and confirm that the deliberate trap is detected. The generated report
   is uploaded as an artifact.

## Scope and limitations

In scope: event-driven simulation, point-in-time data, costs, look-ahead detection, walk-forward,
HTML reporting, deterministic hashing.

Out of scope: broker connections, paid data, portfolio optimisation, margin/borrow costs, dividends,
corporate actions, taxes, intraday calendars and survivorship-bias-free universe construction. The
sample has one symbol; the engine's portfolio and store are multi-symbol, but the example
strategies are single-asset. Slippage is linear in participation, not a square-root impact model.
This is research software, not investment advice.

## License

MIT. Copyright (c) 2026 Iker Abad. See [LICENSE](LICENSE).
