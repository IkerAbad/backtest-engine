"""Real SPY data: detector and walk-forward tests."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest
from backtest_engine import (
    EngineConfig,
    FullSampleZScore,
    FutureAccessError,
    MovingAverage,
    PointInTimeStore,
    check_lookahead,
    run_backtest,
    walk_forward,
)

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data" / "spy_daily.csv"
META = ROOT / "data" / "spy_daily.meta.json"


@pytest.fixture(scope="module")
def store() -> PointInTimeStore:
    return PointInTimeStore.from_csv(str(DATA))


def test_real_data_matches_metadata(store: PointInTimeStore) -> None:
    meta = json.loads(META.read_text(encoding="utf-8"))
    digest = hashlib.sha256(DATA.read_bytes()).hexdigest()
    assert digest == meta["csv_sha256"]
    assert store.bar_count == meta["bars"] == 2514
    assert store.revision_count == 0
    assert store.first_date == "2016-11-21"
    assert store.last_date == "2026-11-20"
    assert store.symbols() == ["SPY"]


def test_honest_strategy_passes_on_real_data(store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: MovingAverage(window=100),
        store,
        EngineConfig(),
        strategy_name="moving average",
    )
    assert report.passed
    assert report.future_accesses == ()
    assert report.divergences == ()


def test_subtle_full_sample_leak_is_detected_on_real_data(store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: FullSampleZScore(),
        store,
        EngineConfig(),
        strategy_name="full-sample z-score",
    )
    assert not report.passed
    assert len(report.future_accesses) == 1
    assert report.future_accesses[0].kind == "research_series"
    assert report.future_accesses[0].future
    assert report.divergences
    first = report.divergences[0]
    assert first.index == 0
    assert first.date == "2016-11-21"
    assert first.normal != first.masked
    assert first.masked == (("SPY", 0.0),)


def test_subtle_strategy_is_rejected_by_default(store: PointInTimeStore) -> None:
    with pytest.raises(FutureAccessError, match="look-ahead denied"):
        run_backtest(FullSampleZScore(), store, EngineConfig())


def test_walk_forward_on_real_data(store: PointInTimeStore) -> None:
    def factory(**params: object) -> MovingAverage:
        return MovingAverage(window=int(str(params["window"])))

    result = walk_forward(
        factory,
        store,
        EngineConfig(),
        train_bars=504,
        test_bars=252,
        step_bars=252,
        grid=[{"window": window} for window in (20, 50, 100, 200)],
    )
    assert len(result.folds) == 7
    for fold in result.folds:
        assert fold.train_end_ts <= fold.test_start_ts
    assert result.total_return == pytest.approx(0.691985, abs=1e-5)
    assert result.sharpe == pytest.approx(0.7980, abs=1e-4)
    assert result.n_trades > 0


def test_committed_case_study_hash() -> None:
    expected = (ROOT / "docs" / "case_study.sha256").read_text(encoding="utf-8").split()
    report = ROOT / expected[1]
    actual = hashlib.sha256(report.read_bytes()).hexdigest()
    assert actual == expected[0]
