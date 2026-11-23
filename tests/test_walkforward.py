"""Walk-forward validation tests."""

from __future__ import annotations

import pytest
from backtest_engine import EngineConfig, MovingAverage, PointInTimeStore, walk_forward

from .helpers import load_store


@pytest.fixture(scope="module")
def store() -> PointInTimeStore:
    return load_store()


def ma_factory(**params: object) -> MovingAverage:
    return MovingAverage(window=int(str(params["window"])))


def test_windows_do_not_overlap(store: PointInTimeStore) -> None:
    result = walk_forward(
        ma_factory,
        store,
        EngineConfig(),
        train_bars=120,
        test_bars=40,
        grid=[{"window": 5}, {"window": 20}],
    )
    assert len(result.folds) > 1
    for fold in result.folds:
        assert fold.train_end_ts <= fold.test_start_ts
        assert fold.test_end_ts > fold.test_start_ts
    for left, right in zip(result.folds, result.folds[1:], strict=False):
        assert left.test_end_ts <= right.test_start_ts


def test_best_params_come_from_the_grid(store: PointInTimeStore) -> None:
    result = walk_forward(
        ma_factory,
        store,
        EngineConfig(),
        train_bars=120,
        test_bars=40,
        grid=[{"window": 5}, {"window": 20}],
    )
    for fold in result.folds:
        assert dict(fold.params)["window"] in {5, 20}
        assert fold.train.metrics.n_bars == 120
        assert fold.test.metrics.n_bars == 40


def test_walkforward_is_deterministic(store: PointInTimeStore) -> None:
    kwargs = {
        "train_bars": 120,
        "test_bars": 40,
        "grid": [{"window": 5}, {"window": 20}],
    }
    first = walk_forward(ma_factory, store, EngineConfig(), **kwargs)
    second = walk_forward(ma_factory, store, EngineConfig(), **kwargs)
    assert first.total_return == second.total_return
    assert first.sharpe == second.sharpe
    assert [fold.params for fold in first.folds] == [fold.params for fold in second.folds]


def test_step_smaller_than_test_is_rejected(store: PointInTimeStore) -> None:
    with pytest.raises(ValueError, match="step_bars"):
        walk_forward(
            ma_factory,
            store,
            EngineConfig(),
            train_bars=120,
            test_bars=40,
            step_bars=20,
            grid=[{"window": 5}],
        )


def test_not_enough_data_is_rejected(store: PointInTimeStore) -> None:
    with pytest.raises(ValueError, match="not enough timestamps"):
        walk_forward(
            ma_factory,
            store,
            EngineConfig(),
            train_bars=700,
            test_bars=200,
            grid=[{"window": 5}],
        )
