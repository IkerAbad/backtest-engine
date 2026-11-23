"""backtest-engine: event-driven backtesting with a look-ahead detector."""

from __future__ import annotations

from ._backtest_engine import (
    BacktestResult,
    Bar,
    Context,
    DataAccess,
    Decision,
    EngineConfig,
    EquityPoint,
    FutureAccessError,
    Metrics,
    Order,
    PointInTimeStore,
    Trade,
    run_backtest,
    sha256_hex,
    version,
)
from .detector import (
    Divergence,
    LookAheadError,
    LookAheadReport,
    check_lookahead,
    run_checked,
)
from .report import write_report
from .strategies import (
    STRATEGIES,
    FullSampleZScore,
    LookAheadTrap,
    MeanReversion,
    MovingAverage,
    Strategy,
)
from .walkforward import Fold, WalkForwardResult, walk_forward

__version__ = version()

__all__ = [
    "STRATEGIES",
    "Bar",
    "BacktestResult",
    "Context",
    "DataAccess",
    "Decision",
    "Divergence",
    "EngineConfig",
    "EquityPoint",
    "Fold",
    "FutureAccessError",
    "FullSampleZScore",
    "LookAheadError",
    "LookAheadReport",
    "LookAheadTrap",
    "MeanReversion",
    "Metrics",
    "MovingAverage",
    "Order",
    "PointInTimeStore",
    "Strategy",
    "Trade",
    "WalkForwardResult",
    "__version__",
    "check_lookahead",
    "run_backtest",
    "run_checked",
    "sha256_hex",
    "walk_forward",
    "write_report",
]
