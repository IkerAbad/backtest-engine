"""Report reproducibility, metrics and cost tests."""

from __future__ import annotations

import hashlib
from pathlib import Path

import pytest
from backtest_engine import (
    EngineConfig,
    MovingAverage,
    PointInTimeStore,
    run_backtest,
    write_report,
)

from .helpers import EXPECTED, REPORT, default_config, load_store, report_verdict


@pytest.fixture(scope="module")
def store() -> PointInTimeStore:
    return load_store()


def _run(store: PointInTimeStore, config: EngineConfig | None = None):
    return run_backtest(MovingAverage(window=20), store, config or default_config())


def test_metrics_are_internally_consistent(store: PointInTimeStore) -> None:
    result = _run(store)
    metrics = result.metrics
    assert metrics.n_bars == 780
    assert metrics.n_trades > 0
    assert metrics.final_equity == result.final_equity
    assert metrics.total_return == metrics.final_equity / metrics.initial_cash - 1.0
    assert metrics.max_drawdown <= 0.0
    assert metrics.turnover > 0.0
    assert metrics.cost_bps == pytest.approx(metrics.total_cost / metrics.initial_cash * 10_000.0)
    assert metrics.commission_cost + metrics.spread_cost + metrics.slippage_cost == (
        metrics.total_cost
    )


def test_costs_lower_the_return(store: PointInTimeStore) -> None:
    with_costs = _run(store)
    without_costs = _run(store, default_config().without_costs())
    assert with_costs.metrics.total_cost > 0.0
    assert without_costs.metrics.total_cost == 0.0
    assert with_costs.final_equity < without_costs.final_equity


def test_report_is_byte_identical(tmp_path: Path, store: PointInTimeStore) -> None:
    result = _run(store)
    first = write_report(
        result,
        tmp_path / "a.html",
        strategy_name="moving average",
        title="Backtest report",
        lookahead_verdict=report_verdict(),
    )
    second = write_report(
        result,
        tmp_path / "b.html",
        strategy_name="moving average",
        title="Backtest report",
        lookahead_verdict=report_verdict(),
    )
    first_bytes = (tmp_path / "a.html").read_bytes()
    second_bytes = (tmp_path / "b.html").read_bytes()
    assert first == second
    assert first_bytes == second_bytes
    assert b"\r\n" not in first_bytes
    digest = hashlib.sha256(first_bytes).hexdigest()
    assert digest == first


def test_report_contains_required_sections(store: PointInTimeStore, tmp_path: Path) -> None:
    result = _run(store)
    path = tmp_path / "report.html"
    write_report(
        result,
        path,
        strategy_name="moving average",
        title="Backtest report",
        lookahead_verdict=report_verdict(),
    )
    html = path.read_text(encoding="utf-8")
    for heading in (
        "Performance",
        "Equity curve",
        "Costs",
        "Configuration",
        "Point-in-time data",
        "Trades",
        "Look-ahead check",
    ):
        assert heading in html
    assert result.data_hash in html
    assert "Total return" in html
    assert "Sharpe ratio" in html
    assert "Max drawdown" in html


def test_committed_example_report_matches_expected_hash() -> None:
    assert EXPECTED.exists(), "reports/expected.sha256 is missing"
    assert REPORT.exists(), "reports/example.html is missing"
    fields = EXPECTED.read_text(encoding="utf-8").split()
    expected = fields[0]
    report_path = Path(fields[1]) if len(fields) > 1 else REPORT
    actual = hashlib.sha256(report_path.read_bytes()).hexdigest()
    assert actual == expected
