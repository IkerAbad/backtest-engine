"""Point-in-time store and execution-model tests."""

from __future__ import annotations

import pytest
from backtest_engine import (
    EngineConfig,
    FutureAccessError,
    LookAheadTrap,
    PointInTimeStore,
    run_backtest,
)

from .helpers import default_config, load_store


@pytest.fixture(scope="module")
def store() -> PointInTimeStore:
    return load_store()


def test_store_counts(store: PointInTimeStore) -> None:
    assert len(store) == 792
    assert store.bar_count == 780
    assert store.revision_count == 12
    assert store.symbols() == ["ACME"]
    assert store.first_date == "2021-01-04"
    assert store.last_date == "2023-12-29"


def test_revision_is_hidden_before_publication(store: PointInTimeStore) -> None:
    revision = next(row for row in store.rows() if row.version == 2)
    original = next(row for row in store.rows() if row.ts == revision.ts and row.version == 1)

    before = store.history_before(revision.symbol, revision.published_at - 1, 10_000)
    after = store.history_before(revision.symbol, revision.published_at, 10_000)
    before_by_ts = {bar.ts: bar for bar in before}
    after_by_ts = {bar.ts: bar for bar in after}

    assert before_by_ts[revision.ts].close == original.close
    assert after_by_ts[revision.ts].close == revision.close
    assert store.as_of(revision.symbol, revision.ts).close == original.close  # type: ignore[union-attr]


def test_initial_bar_is_published_at_its_own_timestamp(store: PointInTimeStore) -> None:
    for bar in store.rows():
        if bar.version == 1:
            assert bar.published_at == bar.ts
        else:
            assert bar.published_at > bar.ts


def test_future_read_is_rejected_by_default(store: PointInTimeStore) -> None:
    with pytest.raises(FutureAccessError, match="look-ahead denied"):
        run_backtest(LookAheadTrap(), store, default_config())


def test_masked_run_hides_every_future_read(store: PointInTimeStore) -> None:
    config = EngineConfig().variant(masked=True, strict_future_access=False)
    result = run_backtest(LookAheadTrap(), store, config)
    assert result.data_accesses
    assert all(access.future for access in result.data_accesses)
    assert all(access.returned_ts is None for access in result.data_accesses)
    assert result.final_equity == result.initial_cash


def test_event_order_is_independent_of_csv_row_order(store: PointInTimeStore) -> None:
    from backtest_engine import MovingAverage

    from .helpers import DATA

    lines = DATA.read_text(encoding="utf-8").splitlines()
    shuffled = "\n".join([lines[0], *reversed(lines[1:])]) + "\n"
    reversed_store = PointInTimeStore.from_csv_str(shuffled)
    assert reversed_store.data_hash == store.data_hash

    config = default_config()
    first = run_backtest(MovingAverage(window=20), store, config)
    second = run_backtest(MovingAverage(window=20), reversed_store, config)
    assert first.final_equity == second.final_equity
    assert first.metrics.sharpe == second.metrics.sharpe
    assert first.data_hash == second.data_hash
