"""Look-ahead detector.

The check runs the same strategy twice from fresh instances:

1. a normal run where the strategy may ask for any timestamp, and
2. a masked run where every request later than the simulated clock returns
   ``None``.

Any future read or any decision that changes between the two runs is a
failure. The report names the first divergent bar and lists the requests that
reached beyond the clock.
"""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass

from ._backtest_engine import (
    BacktestResult,
    DataAccess,
    EngineConfig,
    Order,
    PointInTimeStore,
    run_backtest,
)
from .strategies import Strategy


@dataclass(frozen=True)
class Divergence:
    index: int
    date: str
    symbol: str
    normal: tuple[tuple[str, float], ...]
    masked: tuple[tuple[str, float], ...]


@dataclass(frozen=True)
class LookAheadReport:
    passed: bool
    strategy_name: str
    normal: BacktestResult
    masked: BacktestResult
    future_accesses: tuple[DataAccess, ...]
    divergences: tuple[Divergence, ...]
    execution_diverged: bool
    summary: str


class LookAheadError(RuntimeError):
    """Raised by :func:`run_checked` when the detector finds a violation."""

    def __init__(self, report: LookAheadReport) -> None:
        super().__init__(report.summary)
        self.report = report


def _orders(orders: Sequence[Order]) -> tuple[tuple[str, float], ...]:
    normalized: list[tuple[str, float]] = []
    for order in orders:
        normalized.append((order.symbol, order.target_weight))
    normalized.sort()
    return tuple(normalized)


def _format_orders(orders: tuple[tuple[str, float], ...]) -> str:
    if not orders:
        return "(no orders)"
    return ", ".join(f"{symbol} {weight:+.6f}" for symbol, weight in orders)


def check_lookahead(
    factory: Callable[[], Strategy],
    store: PointInTimeStore,
    config: EngineConfig | None = None,
    *,
    strategy_name: str | None = None,
) -> LookAheadReport:
    """Runs the normal and masked replays and compares their decision traces.

    ``factory`` must return a fresh strategy instance on every call. The two
    runs are otherwise identical: same data, same config, same event order.
    """
    base = config if config is not None else EngineConfig()
    first_strategy = factory()
    name = strategy_name or type(first_strategy).__name__

    normal = run_backtest(
        first_strategy,
        store,
        base.variant(masked=False, strict_future_access=False),
    )
    masked = run_backtest(
        factory(),
        store,
        base.variant(masked=True, strict_future_access=False),
    )

    normal_decisions = normal.decisions
    masked_decisions = masked.decisions
    divergences: list[Divergence] = []
    common = min(len(normal_decisions), len(masked_decisions))
    for index in range(common):
        left = normal_decisions[index]
        right = masked_decisions[index]
        left_orders = _orders(left.orders)
        right_orders = _orders(right.orders)
        if left.ts != right.ts or left.symbol != right.symbol or left_orders != right_orders:
            divergences.append(
                Divergence(
                    index=index,
                    date=left.date,
                    symbol=left.symbol,
                    normal=left_orders,
                    masked=right_orders,
                )
            )
    if len(normal_decisions) != len(masked_decisions):
        longer = (
            normal_decisions if len(normal_decisions) > len(masked_decisions) else masked_decisions
        )
        for index in range(common, len(longer)):
            decision = longer[index]
            divergences.append(
                Divergence(
                    index=index,
                    date=decision.date,
                    symbol=decision.symbol,
                    normal=_orders(normal_decisions[index].orders)
                    if index < len(normal_decisions)
                    else (),
                    masked=_orders(masked_decisions[index].orders)
                    if index < len(masked_decisions)
                    else (),
                )
            )

    future_accesses = tuple(access for access in normal.data_accesses if access.future)
    execution_diverged = normal.final_equity != masked.final_equity or len(normal.trades) != len(
        masked.trades
    )
    passed = not future_accesses and not divergences and not execution_diverged

    lines = ["look-ahead check: " + ("PASSED" if passed else "FAILED")]
    lines.append(f"strategy: {name}")
    lines.append(f"future reads: {len(future_accesses)}")
    if future_accesses:
        first_read = future_accesses[0]
        lines.append(
            "first future read: "
            f"{first_read.clock_date} asked for {first_read.requested_date} "
            f"({first_read.symbol})"
        )
    if divergences:
        first_divergence = divergences[0]
        lines.append(
            f"first divergent decision: #{first_divergence.index} at "
            f"{first_divergence.date} ({first_divergence.symbol})"
        )
        lines.append(f"  normal: {_format_orders(first_divergence.normal)}")
        lines.append(f"  masked: {_format_orders(first_divergence.masked)}")
        lines.append(f"divergent decisions: {len(divergences)}")
    if execution_diverged:
        lines.append(
            "execution diverged: "
            f"normal equity={normal.final_equity:.6f} "
            f"masked equity={masked.final_equity:.6f}"
        )
    if passed:
        lines.append("masked replay reproduced every decision and every fill")
    elif not future_accesses and not divergences:
        lines.append("no future request was logged, but the executions differ")
    summary = "\n".join(lines)

    return LookAheadReport(
        passed=passed,
        strategy_name=name,
        normal=normal,
        masked=masked,
        future_accesses=future_accesses,
        divergences=tuple(divergences),
        execution_diverged=execution_diverged,
        summary=summary,
    )


def run_checked(
    factory: Callable[[], Strategy],
    store: PointInTimeStore,
    config: EngineConfig | None = None,
    *,
    strategy_name: str | None = None,
) -> LookAheadReport:
    """Like :func:`check_lookahead`, but raises :class:`LookAheadError` on failure."""
    report = check_lookahead(
        factory,
        store,
        config,
        strategy_name=strategy_name,
    )
    if not report.passed:
        raise LookAheadError(report)
    return report
