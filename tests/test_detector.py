"""Acceptance tests for the look-ahead detector."""

from __future__ import annotations

import pytest
from backtest_engine import (
    Context,
    LookAheadError,
    LookAheadTrap,
    MeanReversion,
    MovingAverage,
    Order,
    PointInTimeStore,
    check_lookahead,
    run_checked,
)
from backtest_engine._backtest_engine import Bar

from .helpers import default_config, load_store


@pytest.fixture(scope="module")
def store() -> PointInTimeStore:
    return load_store()


def test_moving_average_passes(store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: MovingAverage(window=20),
        store,
        default_config(),
        strategy_name="moving average",
    )
    assert report.passed
    assert report.future_accesses == ()
    assert report.divergences == ()
    assert "PASSED" in report.summary


def test_mean_reversion_passes(store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: MeanReversion(window=20, entry_z=1.0),
        store,
        default_config(),
    )
    assert report.passed


def test_trap_is_detected_with_location(store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: LookAheadTrap(),
        store,
        default_config(),
        strategy_name="trap",
    )
    assert not report.passed
    assert report.future_accesses
    assert report.divergences
    first = report.divergences[0]
    assert first.index == 0
    assert first.date == "2021-01-04"
    assert first.normal == (("ACME", 1.0),)
    assert first.masked == (("ACME", 0.0),)
    assert "FAILED" in report.summary
    assert "first divergent decision" in report.summary
    assert "normal: ACME +1.000000" in report.summary
    assert "masked: ACME +0.000000" in report.summary


def test_run_checked_raises_on_trap(store: PointInTimeStore) -> None:
    with pytest.raises(LookAheadError) as caught:
        run_checked(lambda: LookAheadTrap(), store, default_config())
    assert caught.value.report.divergences
    assert "look-ahead check: FAILED" in str(caught.value)


def test_detector_catches_non_deterministic_state(store: PointInTimeStore) -> None:
    global _RUNS
    _RUNS = 0

    class Unstable:
        def __init__(self) -> None:
            global _RUNS
            _RUNS += 1
            self._run = _RUNS

        def on_bar(self, ctx: Context, bar: Bar) -> list[Order]:
            weight = 1.0 if self._run == 1 else 0.0
            return [ctx.order(bar.symbol, weight)]

    report = check_lookahead(lambda: Unstable(), store, default_config())
    assert not report.passed
    assert report.future_accesses == ()
    assert report.divergences
    assert report.execution_diverged
