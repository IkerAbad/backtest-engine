"""Demonstrate that the detector rejects the deliberate look-ahead strategy.

Exit code 0 means the trap was detected (the expected outcome). Exit code 1
means the detector failed to catch it, which is a bug.
"""

from __future__ import annotations

from backtest_engine import EngineConfig, LookAheadTrap, PointInTimeStore, check_lookahead


def main() -> int:
    store = PointInTimeStore.from_csv("data/sample.csv")
    report = check_lookahead(
        lambda: LookAheadTrap(),
        store,
        EngineConfig(),
        strategy_name="Look-ahead trap",
    )
    print(report.summary)
    if report.passed:
        print("\nERROR: the trap was not detected")
        return 1
    print("\nDetection confirmed. This is the expected failing report.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
