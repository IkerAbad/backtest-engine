"""Multi-asset strategies, allocation, per-asset costs and detector coverage."""

from __future__ import annotations

import hashlib
from pathlib import Path

import pytest
from backtest_engine import (
    EngineConfig,
    FutureAccessError,
    MultiAssetFutureTrap,
    Order,
    PointInTimeStore,
    RelativeMomentum,
    check_lookahead,
    run_backtest,
    walk_forward,
)
from backtest_engine._backtest_engine import Bar, Context

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data" / "spy_tlt_daily.csv"

CSV = """symbol,ts,published_at,version,open,high,low,close,volume
SPY,2024-01-02T21:00:00Z,2024-01-02T21:00:00Z,1,10,10,10,10,1000000
TLT,2024-01-02T21:00:00Z,2024-01-02T21:00:00Z,1,20,20,20,20,1000000
SPY,2024-01-03T21:00:00Z,2024-01-03T21:00:00Z,1,10,11,10,11,1000000
TLT,2024-01-03T21:00:00Z,2024-01-03T21:00:00Z,1,20,20,20,20,1000000
SPY,2024-01-04T21:00:00Z,2024-01-04T21:00:00Z,1,11,11,11,11,1000000
TLT,2024-01-04T21:00:00Z,2024-01-04T21:00:00Z,1,20,20,20,20,1000000
"""


class TwoAssetTargets:
    """Emit both legs from every callback at the first timestamp."""

    def __init__(self) -> None:
        self._first_ts: int | None = None

    def on_bar(self, ctx: Context, _bar: Bar) -> list[Order]:
        if self._first_ts is None:
            self._first_ts = ctx.now
        if ctx.now != self._first_ts:
            return []
        return [
            ctx.order("SPY", 0.6),
            ctx.order("TLT", 0.4),
        ]


@pytest.fixture(scope="module")
def real_store() -> PointInTimeStore:
    return PointInTimeStore.from_csv(str(DATA))


def test_multi_asset_allocation_and_deduplication() -> None:
    store = PointInTimeStore.from_csv_str(CSV)
    result = run_backtest(TwoAssetTargets(), store, EngineConfig().without_costs())
    assert len(result.trades) == 2
    by_symbol = {trade.symbol: trade for trade in result.trades}
    assert set(by_symbol) == {"SPY", "TLT"}
    assert by_symbol["SPY"].notional == pytest.approx(600_000.0)
    assert by_symbol["TLT"].notional == pytest.approx(400_000.0)
    assert result.final_equity == pytest.approx(1_060_000.0)


def test_commissions_are_charged_per_asset() -> None:
    store = PointInTimeStore.from_csv_str(CSV)
    config = EngineConfig(commission_bps=1.0, commission_fixed=1.0).variant(
        masked=False, strict_future_access=True
    )
    result = run_backtest(TwoAssetTargets(), store, config)
    by_symbol = {trade.symbol: trade for trade in result.trades}
    assert by_symbol["SPY"].commission == pytest.approx(61.0)
    assert by_symbol["TLT"].commission == pytest.approx(41.0)
    assert result.metrics.commission_cost == pytest.approx(102.0)


def test_relative_momentum_passes_on_real_data(real_store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: RelativeMomentum(lookback=126),
        real_store,
        EngineConfig(),
        strategy_name="relative momentum",
    )
    assert report.passed
    assert report.future_accesses == ()
    assert report.divergences == ()


def test_multi_asset_trap_is_detected_on_real_data(real_store: PointInTimeStore) -> None:
    report = check_lookahead(
        lambda: MultiAssetFutureTrap(),
        real_store,
        EngineConfig(),
        strategy_name="multi-asset trap",
    )
    assert not report.passed
    assert report.future_accesses
    assert report.divergences
    first = report.divergences[0]
    assert first.index == 0
    assert first.date == "2016-11-21"
    assert first.masked == (("SPY", 0.0), ("TLT", 0.0))
    assert first.normal != first.masked
    assert "FAILED" in report.summary


def test_multi_asset_trap_is_rejected_by_default(real_store: PointInTimeStore) -> None:
    with pytest.raises(FutureAccessError, match="look-ahead denied"):
        run_backtest(MultiAssetFutureTrap(), real_store, EngineConfig())


def test_multi_asset_walk_forward(real_store: PointInTimeStore) -> None:
    def factory(**params: object) -> RelativeMomentum:
        return RelativeMomentum(lookback=int(str(params["lookback"])))

    result = walk_forward(
        factory,
        real_store,
        EngineConfig(),
        train_bars=504,
        test_bars=252,
        step_bars=252,
        grid=[{"lookback": lookback} for lookback in (63, 126, 252)],
    )
    assert len(result.folds) == 7
    for fold in result.folds:
        assert fold.train_end_ts <= fold.test_start_ts
        assert dict(fold.params)["lookback"] in {63, 126, 252}
    assert result.n_trades > 0


def test_committed_multi_asset_case_hash() -> None:
    expected = (ROOT / "docs" / "multi_asset.sha256").read_text(encoding="utf-8").split()
    report = ROOT / expected[1]
    actual = hashlib.sha256(report.read_bytes()).hexdigest()
    assert actual == expected[0]
