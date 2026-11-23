"""Reference strategies for the engine.

All three strategies use only the point-in-time API. The third one is a
deliberate cheat: it reads the next bar to decide today's order. The detector
must catch it.
"""

from __future__ import annotations

import math
from collections import deque
from typing import Protocol

from ._backtest_engine import Bar, Context, Order


class Strategy(Protocol):
    """Minimal strategy protocol.

    `on_start` and `on_finish` are optional. State lives explicitly on the
    instance, never in module globals.
    """

    def on_bar(self, ctx: Context, bar: Bar) -> list[Order]: ...


class MovingAverage:
    """Long when the close is above its trailing moving average, flat otherwise."""

    def __init__(self, window: int = 20) -> None:
        if window < 1:
            raise ValueError("window must be >= 1")
        self.window = window
        self._closes: deque[float] = deque(maxlen=window)

    def on_bar(self, ctx: Context, bar: Bar) -> list[Order]:
        self._closes.append(bar.close)
        if len(self._closes) < self.window:
            return []
        total = 0.0
        for value in self._closes:
            total += value
        average = total / self.window
        weight = 1.0 if bar.close > average else 0.0
        return [ctx.order(bar.symbol, weight)]


class MeanReversion:
    """Buy oversold closes and exit when the z-score returns to zero."""

    def __init__(self, window: int = 20, entry_z: float = 1.0, exit_z: float = 0.0) -> None:
        if window < 2:
            raise ValueError("window must be >= 2")
        if entry_z <= 0.0:
            raise ValueError("entry_z must be positive")
        if exit_z > entry_z:
            raise ValueError("exit_z must be <= entry_z")
        self.window = window
        self.entry_z = entry_z
        self.exit_z = exit_z
        self._closes: deque[float] = deque(maxlen=window)
        self._weight = 0.0

    def on_bar(self, ctx: Context, bar: Bar) -> list[Order]:
        self._closes.append(bar.close)
        if len(self._closes) < self.window:
            return []
        mean = 0.0
        for value in self._closes:
            mean += value
        mean /= self.window
        variance = 0.0
        for value in self._closes:
            delta = value - mean
            variance += delta * delta
        variance /= self.window
        stddev = math.sqrt(variance)
        if stddev == 0.0:
            return []
        zscore = (bar.close - mean) / stddev
        previous = self._weight
        if zscore <= -self.entry_z:
            self._weight = 1.0
        elif zscore >= self.exit_z:
            self._weight = 0.0
        if self._weight == previous:
            return []
        return [ctx.order(bar.symbol, self._weight)]


class LookAheadTrap:
    """Deliberate look-ahead: trade today on tomorrow's close.

    This strategy exists to prove that the detector works. It must never be
    used as an example of an honest strategy.
    """

    def __init__(self, horizon_seconds: int = 86_400) -> None:
        if horizon_seconds <= 0:
            raise ValueError("horizon_seconds must be positive")
        self.horizon_seconds = horizon_seconds

    def on_bar(self, ctx: Context, bar: Bar) -> list[Order]:
        future = ctx.as_of(bar.symbol, ctx.now + self.horizon_seconds)
        if future is None:
            return [ctx.order(bar.symbol, 0.0)]
        weight = 1.0 if future.close > bar.close else -1.0
        return [ctx.order(bar.symbol, weight)]


class FullSampleZScore:
    """Subtle look-ahead: normalize with statistics of the whole sample.

    This is the classic "I passed the full DataFrame to the strategy" mistake,
    expressed through the engine's audited research view. The mean and standard
    deviation include rows published after the current clock. In the normal run
    the strategy gets the full series; in the masked replay it only gets rows
    visible at the clock, so the first standard deviation is zero and the
    decisions change. The detector must flag it.
    """

    def __init__(self, scale: float = 0.5) -> None:
        if scale <= 0.0:
            raise ValueError("scale must be positive")
        self.scale = scale
        self._mean: float | None = None
        self._stddev: float | None = None

    def _fit(self, ctx: Context, symbol: str) -> None:
        bars = ctx.research_series(symbol)
        closes = [bar.close for bar in bars]
        if not closes:
            self._mean = 0.0
            self._stddev = 0.0
            return
        total = 0.0
        for value in closes:
            total += value
        mean = total / len(closes)
        variance = 0.0
        for value in closes:
            delta = value - mean
            variance += delta * delta
        self._mean = mean
        self._stddev = math.sqrt(variance / len(closes))

    def on_bar(self, ctx: Context, bar: Bar) -> list[Order]:
        if self._mean is None or self._stddev is None:
            self._fit(ctx, bar.symbol)
        assert self._mean is not None and self._stddev is not None
        if self._stddev == 0.0:
            return [ctx.order(bar.symbol, 0.0)]
        zscore = (bar.close - self._mean) / self._stddev
        weight = max(-1.0, min(1.0, -self.scale * zscore))
        return [ctx.order(bar.symbol, weight)]


STRATEGIES = {
    "moving_average": MovingAverage,
    "mean_reversion": MeanReversion,
    "lookahead_trap": LookAheadTrap,
    "full_sample_zscore": FullSampleZScore,
}
