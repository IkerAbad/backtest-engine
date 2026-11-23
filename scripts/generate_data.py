"""Generate the committed deterministic sample dataset.

The generator uses only integer LCG arithmetic and IEEE-754 basic operations,
so the CSV can be reproduced without NumPy or any platform-dependent library.
The output is committed; CI never regenerates it.

Usage:
    python scripts/generate_data.py
"""

from __future__ import annotations

import argparse
import csv
from datetime import date, timedelta
from pathlib import Path

MASK = (1 << 64) - 1
MULTIPLIER = 6364136223846793005
INCREMENT = 1442695040888963407


class Lcg:
    def __init__(self, seed: int) -> None:
        self.state = seed & MASK

    def next_u64(self) -> int:
        self.state = (self.state * MULTIPLIER + INCREMENT) & MASK
        return self.state

    def uniform(self) -> float:
        # 53 random bits, exactly representable in an f64.
        return (self.next_u64() >> 11) / float(1 << 53)

    def normal(self) -> float:
        # Irwin-Hall approximation: sum of 12 uniforms minus 6.
        return sum(self.uniform() for _ in range(12)) - 6.0


def business_days(start: date, count: int) -> list[date]:
    out: list[date] = []
    current = start
    while len(out) < count:
        if current.weekday() < 5:
            out.append(current)
        current += timedelta(days=1)
    return out


def iso(day: date, hour: int = 21) -> str:
    return f"{day.isoformat()}T{hour:02d}:00:00Z"


def fmt(value: float) -> str:
    return f"{value:.4f}"


def build_rows(bars: int, seed: int) -> list[dict[str, object]]:
    rng = Lcg(seed)
    days = business_days(date(2021, 1, 4), bars)
    closes: list[float] = []
    rows: list[dict[str, object]] = []
    price = 100.0

    for index, day in enumerate(days):
        if index == 0:
            previous = price
            ret = 0.0
        else:
            previous = closes[-1]
            lookback = closes[-10] if len(closes) >= 10 else closes[0]
            momentum = (previous / lookback - 1.0) / 10.0
            regime = 0.0006 if (index // 120) % 2 == 0 else -0.00035
            shock = 0.0
            if index % 137 == 0:
                shock = -0.025
            elif index % 211 == 0:
                shock = 0.018
            ret = regime + 0.35 * momentum + 0.0105 * rng.normal() + shock
            ret = max(-0.08, min(0.08, ret))

        close = previous * (1.0 + ret) if index > 0 else previous
        close = max(5.0, close)
        gap = 0.003 * rng.normal() if index > 0 else 0.0
        open_price = previous * (1.0 + gap)
        open_price = max(5.0, open_price)

        high = max(open_price, close) * (1.0 + 0.0045 * abs(rng.normal()))
        low = min(open_price, close) * (1.0 - 0.0045 * abs(rng.normal()))
        volume = round(1_000_000.0 * (0.55 + 1.1 * rng.uniform()))

        open_price = round(open_price, 4)
        close = round(close, 4)
        high = round(max(high, open_price, close), 4)
        low = round(min(low, open_price, close), 4)

        timestamp = iso(day)
        rows.append(
            {
                "symbol": "ACME",
                "ts": timestamp,
                "published_at": timestamp,
                "version": 1,
                "open": open_price,
                "high": high,
                "low": low,
                "close": close,
                "volume": volume,
            }
        )
        closes.append(close)

        # Restatements: the row is republished later with small corrections.
        if index > 0 and index % 97 == 0:
            factor = 1.0015 if (index // 97) % 2 else 0.9985
            later = day + timedelta(days=2)
            rows.append(
                {
                    "symbol": "ACME",
                    "ts": timestamp,
                    "published_at": iso(later),
                    "version": 2,
                    "open": round(open_price * factor, 4),
                    "high": round(high * factor, 4),
                    "low": round(low * factor, 4),
                    "close": round(close * factor, 4),
                    "volume": round(volume * 1.03),
                }
            )
        elif index > 0 and index % 173 == 0:
            # A slower correction published one month later.
            factor = 0.999 if (index // 173) % 2 else 1.001
            later = day + timedelta(days=30)
            rows.append(
                {
                    "symbol": "ACME",
                    "ts": timestamp,
                    "published_at": iso(later),
                    "version": 2,
                    "open": round(open_price * factor, 4),
                    "high": round(high * factor, 4),
                    "low": round(low * factor, 4),
                    "close": round(close * factor, 4),
                    "volume": round(volume / 1.03),
                }
            )

    return rows


def sort_key(row: dict[str, object]) -> tuple[str, str, int]:
    return (str(row["ts"]), str(row["published_at"]), int(str(row["version"])))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default="data/sample.csv")
    parser.add_argument("--bars", type=int, default=780)
    parser.add_argument("--seed", type=int, default=20260101)
    args = parser.parse_args()

    rows = sorted(build_rows(args.bars, args.seed), key=sort_key)
    path = Path(args.out)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[
                "symbol",
                "ts",
                "published_at",
                "version",
                "open",
                "high",
                "low",
                "close",
                "volume",
            ],
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)
    revisions = sum(1 for row in rows if int(str(row["version"])) > 1)
    print(f"wrote {len(rows)} rows ({revisions} revisions) to {path}")


if __name__ == "__main__":
    main()
