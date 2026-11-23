"""Generate the committed example report and print its hash.

Run from the repository root after `maturin develop --release` (or after
`pip install .`).
"""

from __future__ import annotations

from pathlib import Path

from backtest_engine import (
    EngineConfig,
    MovingAverage,
    PointInTimeStore,
    run_backtest,
    run_checked,
    write_report,
)


def main() -> int:
    store = PointInTimeStore.from_csv("data/sample.csv")
    config = EngineConfig()
    window = 20
    factory = lambda: MovingAverage(window=window)  # noqa: E731
    label = f"Moving average (window={window})"

    check = run_checked(factory, store, config, strategy_name=label)
    verdict = (
        "PASSED - masked replay reproduced every decision; "
        f"{len(check.future_accesses)} future reads"
    )

    result = run_backtest(factory(), store, config)
    path = Path("reports/example.html")
    digest = write_report(
        result,
        path,
        strategy_name=label,
        title="Backtest report - moving average",
        lookahead_verdict=verdict,
    )
    metrics = result.metrics
    print(f"strategy: {label}")
    print(f"total return: {metrics.total_return * 100:.6f}%")
    print(f"annualized volatility: {metrics.annualized_volatility * 100:.6f}%")
    print(f"sharpe: {metrics.sharpe:.6f}")
    print(f"max drawdown: {metrics.max_drawdown * 100:.6f}%")
    print(f"turnover: {metrics.turnover:.6f}")
    print(f"total cost: {metrics.total_cost:.2f}")
    print(f"fill count: {metrics.n_trades}")
    print(f"final equity: {metrics.final_equity:.2f}")
    print(f"report: {path}")
    print(f"sha256: {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
