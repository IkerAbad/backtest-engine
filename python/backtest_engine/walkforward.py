"""Walk-forward validation with non-overlapping train and test windows.

For each fold, every parameter set is evaluated only on the training window.
The winner (highest Sharpe, ties broken by total return and grid order) is then
run once on the following test window. Test windows never overlap and always
start after the training window ends.
"""

from __future__ import annotations

import math
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass

from ._backtest_engine import BacktestResult, EngineConfig, PointInTimeStore, run_backtest
from .strategies import Strategy


@dataclass(frozen=True)
class Fold:
    index: int
    train_start_ts: int
    train_end_ts: int
    test_start_ts: int
    test_end_ts: int
    params: tuple[tuple[str, object], ...]
    train: BacktestResult
    test: BacktestResult


@dataclass(frozen=True)
class WalkForwardResult:
    folds: tuple[Fold, ...]
    total_return: float
    annualized_volatility: float
    sharpe: float
    max_drawdown: float
    total_cost: float
    n_trades: int


def _aggregate(folds: Sequence[Fold], periods_per_year: int) -> WalkForwardResult:
    returns: list[float] = []
    for fold in folds:
        points = fold.test.equity
        for index in range(1, len(points)):
            previous = points[index - 1].equity
            if previous > 0.0:
                returns.append(points[index].equity / previous - 1.0)

    wealth = 1.0
    peak = 1.0
    max_drawdown = 0.0
    for value in returns:
        wealth *= 1.0 + value
        if wealth > peak:
            peak = wealth
        drawdown = wealth / peak - 1.0
        if drawdown < max_drawdown:
            max_drawdown = drawdown

    if len(returns) >= 2:
        mean = sum(returns) / len(returns)
        squared = 0.0
        for value in returns:
            delta = value - mean
            squared += delta * delta
        variance = squared / (len(returns) - 1)
        stddev = math.sqrt(variance)
        annualized_volatility = math.sqrt(variance * periods_per_year)
        sharpe = mean / stddev * math.sqrt(periods_per_year) if stddev > 0.0 else 0.0
    else:
        annualized_volatility = 0.0
        sharpe = 0.0

    return WalkForwardResult(
        folds=tuple(folds),
        total_return=wealth - 1.0,
        annualized_volatility=annualized_volatility,
        sharpe=sharpe,
        max_drawdown=max_drawdown,
        total_cost=sum(fold.test.metrics.total_cost for fold in folds),
        n_trades=sum(fold.test.metrics.n_trades for fold in folds),
    )


def walk_forward(
    factory: Callable[..., Strategy],
    store: PointInTimeStore,
    config: EngineConfig | None = None,
    *,
    train_bars: int = 252,
    test_bars: int = 63,
    step_bars: int | None = None,
    grid: Sequence[Mapping[str, object]] | None = None,
) -> WalkForwardResult:
    """Runs walk-forward validation.

    ``factory`` is called with the keyword arguments of each grid entry and
    must return a fresh strategy. ``step_bars`` defaults to ``test_bars``, so
    test windows are contiguous and never overlap. It may be larger, leaving
    gaps between test windows; it may not be smaller.
    """
    base = config if config is not None else EngineConfig()
    timestamps = store.timestamps()
    if not timestamps:
        raise ValueError("store contains no events")
    if train_bars < 1 or test_bars < 1:
        raise ValueError("train_bars and test_bars must be >= 1")
    step = step_bars if step_bars is not None else test_bars
    if step < test_bars:
        raise ValueError("step_bars must be >= test_bars so test windows do not overlap")

    combos = list(grid) if grid is not None else [{}]
    if not combos:
        raise ValueError("grid must contain at least one parameter set")

    starts = range(0, len(timestamps) - train_bars - test_bars + 1, step)
    folds: list[Fold] = []
    for fold_index, start in enumerate(starts):
        train_end = start + train_bars
        test_end = train_end + test_bars
        train_start_ts = timestamps[start]
        train_end_ts = timestamps[train_end - 1] + 1
        test_start_ts = timestamps[train_end]
        test_end_ts = timestamps[test_end - 1] + 1

        train_config = base.variant(masked=False, strict_future_access=True).with_window(
            train_start_ts, train_end_ts
        )
        test_config = base.variant(masked=False, strict_future_access=True).with_window(
            test_start_ts, test_end_ts
        )

        best_params: Mapping[str, object] | None = None
        best_train: BacktestResult | None = None
        best_score: tuple[float, float] | None = None
        for params in combos:
            train_result = run_backtest(factory(**dict(params)), store, train_config)
            score = (train_result.metrics.sharpe, train_result.metrics.total_return)
            if best_score is None or score > best_score:
                best_score = score
                best_params = params
                best_train = train_result

        assert best_params is not None and best_train is not None
        test_result = run_backtest(factory(**dict(best_params)), store, test_config)
        folds.append(
            Fold(
                index=fold_index,
                train_start_ts=train_start_ts,
                train_end_ts=train_end_ts,
                test_start_ts=test_start_ts,
                test_end_ts=test_end_ts,
                params=tuple(sorted(best_params.items())),
                train=best_train,
                test=test_result,
            )
        )

    if not folds:
        raise ValueError("not enough timestamps for one train/test window")
    return _aggregate(folds, base.periods_per_year)
