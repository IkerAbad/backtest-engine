"""Command line interface.

python -m backtest_engine backtest --strategy moving_average --window 20
python -m backtest_engine check --strategy lookahead_trap --expect-detected
python -m backtest_engine walkforward --strategy moving_average --windows 5,10,20,50
"""

from __future__ import annotations

import argparse
from collections.abc import Callable, Sequence
from datetime import datetime, timezone

from ._backtest_engine import (
    BacktestResult,
    EngineConfig,
    PointInTimeStore,
    run_backtest,
)
from .detector import check_lookahead
from .report import write_report
from .strategies import (
    STRATEGIES,
    FullSampleZScore,
    LookAheadTrap,
    MeanReversion,
    MovingAverage,
    MultiAssetFutureTrap,
    RelativeMomentum,
    Strategy,
)
from .walkforward import WalkForwardResult, walk_forward


def _factory(name: str, args: argparse.Namespace) -> Callable[[], Strategy]:
    if name == "moving_average":
        window = int(args.window)
        return lambda: MovingAverage(window=window)
    if name == "mean_reversion":
        window = int(args.window)
        entry_z = float(args.entry_z)
        return lambda: MeanReversion(window=window, entry_z=entry_z)
    if name == "lookahead_trap":
        horizon = int(args.horizon_days) * 86_400
        return lambda: LookAheadTrap(horizon_seconds=horizon)
    if name == "full_sample_zscore":
        return lambda: FullSampleZScore()
    if name == "relative_momentum":
        lookback = int(args.window)
        return lambda: RelativeMomentum(lookback=lookback)
    if name == "multi_asset_trap":
        horizon = int(args.horizon_days) * 86_400
        return lambda: MultiAssetFutureTrap(horizon_seconds=horizon)
    raise ValueError(f"unknown strategy {name!r}")


def _config(args: argparse.Namespace) -> EngineConfig:
    config = EngineConfig(
        initial_cash=float(args.initial_cash),
        commission_bps=float(args.commission_bps),
        spread_bps=float(args.spread_bps),
        slippage_bps_at_full_volume=float(args.slippage_bps),
        periods_per_year=int(args.periods_per_year),
    )
    if bool(args.no_costs):
        config = config.without_costs()
    return config


def _strategy_label(name: str, args: argparse.Namespace) -> str:
    if name == "moving_average":
        return f"Moving average (window={int(args.window)})"
    if name == "mean_reversion":
        return f"Mean reversion (window={int(args.window)}, entry_z={float(args.entry_z):g})"
    if name == "lookahead_trap":
        return f"Look-ahead trap (horizon={int(args.horizon_days)}d)"
    if name == "full_sample_zscore":
        return "Full-sample z-score (subtle look-ahead)"
    if name == "relative_momentum":
        return f"Relative momentum (lookback={int(args.window)})"
    if name == "multi_asset_trap":
        return "Multi-asset future trap"
    return name


def _print_result(result: BacktestResult) -> None:
    metrics = result.metrics
    print(f"  total return          {metrics.total_return * 100:.4f}%")
    print(f"  annualized volatility {metrics.annualized_volatility * 100:.4f}%")
    print(f"  Sharpe                {metrics.sharpe:.4f}")
    print(f"  max drawdown          {metrics.max_drawdown * 100:.4f}%")
    print(f"  turnover              {metrics.turnover:.4f}x")
    print(f"  total cost            {metrics.total_cost:.2f} ({metrics.cost_bps:.2f} bps)")
    print(f"  fills                 {metrics.n_trades}")
    print(f"  final equity          {metrics.final_equity:.2f}")


def _add_common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--data", default="data/sample.csv", help="point-in-time CSV")
    parser.add_argument(
        "--strategy",
        choices=sorted(STRATEGIES),
        default="moving_average",
        help="strategy to run",
    )
    parser.add_argument("--window", type=int, default=20)
    parser.add_argument("--entry-z", type=float, default=1.0)
    parser.add_argument("--horizon-days", type=int, default=1)
    parser.add_argument("--initial-cash", type=float, default=1_000_000.0)
    parser.add_argument("--commission-bps", type=float, default=1.0)
    parser.add_argument("--spread-bps", type=float, default=2.0)
    parser.add_argument("--slippage-bps", type=float, default=100.0)
    parser.add_argument("--periods-per-year", type=int, default=252)
    parser.add_argument(
        "--no-costs",
        action="store_true",
        help="zero commission, spread and slippage",
    )


def _add_backtest(parser: argparse.ArgumentParser) -> None:
    _add_common(parser)
    parser.add_argument("--report", default="reports/example.html")
    parser.add_argument("--title", default=None)
    parser.add_argument(
        "--no-check",
        action="store_true",
        help="skip the masked look-ahead replay (debug only)",
    )


def _add_check(parser: argparse.ArgumentParser) -> None:
    _add_common(parser)
    parser.add_argument(
        "--expect-detected",
        action="store_true",
        help="exit 0 only if the detector rejects the strategy",
    )


def _add_walkforward(parser: argparse.ArgumentParser) -> None:
    _add_common(parser)
    parser.add_argument("--train", type=int, default=252)
    parser.add_argument("--test", type=int, default=63)
    parser.add_argument("--step", type=int, default=None)
    parser.add_argument("--windows", default="5,10,20,50")


def _cmd_backtest(args: argparse.Namespace) -> int:
    store = PointInTimeStore.from_csv(str(args.data))
    config = _config(args)
    name = str(args.strategy)
    factory = _factory(name, args)
    label = _strategy_label(name, args)

    if bool(args.no_check):
        verdict = "NOT RUN - the report was generated without a masked replay"
    else:
        check = check_lookahead(factory, store, config, strategy_name=label)
        if not check.passed:
            print(check.summary)
            print("refusing to write a performance report for a rejected strategy")
            return 1
        verdict = "PASSED - masked replay reproduced every decision; 0 future reads"

    result = run_backtest(factory(), store, config)
    title = str(args.title) if args.title is not None else f"Backtest report - {label}"
    digest = write_report(
        result,
        str(args.report),
        strategy_name=label,
        title=title,
        lookahead_verdict=verdict,
    )
    print(f"strategy: {label}")
    _print_result(result)
    print(f"report: {args.report}")
    print(f"sha256: {digest}")
    return 0


def _cmd_check(args: argparse.Namespace) -> int:
    store = PointInTimeStore.from_csv(str(args.data))
    config = _config(args)
    name = str(args.strategy)
    factory = _factory(name, args)
    report = check_lookahead(
        factory,
        store,
        config,
        strategy_name=_strategy_label(name, args),
    )
    print(report.summary)
    detected = not report.passed
    if bool(args.expect_detected):
        if detected:
            print("expected detection: CONFIRMED")
            return 0
        print("expected detection: FAILED - the strategy was not flagged")
        return 1
    return 1 if detected else 0


def _date(ts: int) -> str:
    return datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%d")


def _print_walkforward(result: WalkForwardResult) -> None:
    print(
        "fold  train_start  train_end    test_start   test_end     best params"
        "                    test return  test Sharpe"
    )
    for fold in result.folds:
        params = ", ".join(f"{key}={value}" for key, value in fold.params)
        print(
            f"{fold.index:>4}  {_date(fold.train_start_ts)}   {_date(fold.train_end_ts)}   "
            f"{_date(fold.test_start_ts)}   {_date(fold.test_end_ts)}   {params:<42}  "
            f"{fold.test.metrics.total_return * 100:>10.4f}%  {fold.test.metrics.sharpe:>11.4f}"
        )
    print()
    print("aggregate out-of-sample:")
    print(f"  folds                 {len(result.folds)}")
    print(f"  total return          {result.total_return * 100:.4f}%")
    print(f"  annualized volatility {result.annualized_volatility * 100:.4f}%")
    print(f"  Sharpe                {result.sharpe:.4f}")
    print(f"  max drawdown          {result.max_drawdown * 100:.4f}%")
    print(f"  total cost            {result.total_cost:.2f}")
    print(f"  fills                 {result.n_trades}")


def _cmd_walkforward(args: argparse.Namespace) -> int:
    store = PointInTimeStore.from_csv(str(args.data))
    config = _config(args)
    name = str(args.strategy)
    if name == "lookahead_trap":
        print("walk-forward is not meaningful for the deliberate look-ahead trap")
        return 2
    if name == "full_sample_zscore":
        print("walk-forward is not meaningful for the full-sample look-ahead strategy")
        return 2
    if name == "multi_asset_trap":
        print("walk-forward is not meaningful for the multi-asset future trap")
        return 2
    windows = [int(part) for part in str(args.windows).split(",") if part.strip()]
    if not windows:
        print("--windows must contain at least one integer")
        return 2
    if name == "moving_average":
        grid: list[dict[str, object]] = [{"window": window} for window in windows]

        def ma_factory(**params: object) -> Strategy:
            return MovingAverage(window=int(str(params["window"])))

        factory: Callable[..., Strategy] = ma_factory
    elif name == "relative_momentum":
        grid = [{"lookback": lookback} for lookback in windows]

        def rm_factory(**params: object) -> Strategy:
            return RelativeMomentum(lookback=int(str(params["lookback"])))

        factory = rm_factory
    else:
        entry_z = float(args.entry_z)
        grid = [{"window": window, "entry_z": entry_z} for window in windows]

        def mr_factory(**params: object) -> Strategy:
            return MeanReversion(
                window=int(str(params["window"])),
                entry_z=float(str(params["entry_z"])),
            )

        factory = mr_factory

    result = walk_forward(
        factory,
        store,
        config,
        train_bars=int(args.train),
        test_bars=int(args.test),
        step_bars=int(args.step) if args.step is not None else None,
        grid=grid,
    )
    _print_walkforward(result)
    return 0


def make_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="backtest-engine",
        description="Event-driven backtester with a look-ahead detector.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    _add_backtest(subparsers.add_parser("backtest", help="run a backtest and write HTML"))
    _add_check(subparsers.add_parser("check", help="run only the look-ahead detector"))
    _add_walkforward(
        subparsers.add_parser("walkforward", help="run non-overlapping train/test folds")
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = make_parser()
    args = parser.parse_args(argv)
    command = str(args.command)
    if command == "backtest":
        return _cmd_backtest(args)
    if command == "check":
        return _cmd_check(args)
    if command == "walkforward":
        return _cmd_walkforward(args)
    parser.error(f"unknown command {command!r}")
    return 2
