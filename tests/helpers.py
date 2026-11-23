"""Shared test helpers."""

from __future__ import annotations

from pathlib import Path

from backtest_engine import EngineConfig, PointInTimeStore

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data" / "sample.csv"
REPORT = ROOT / "reports" / "example.html"
EXPECTED = ROOT / "reports" / "expected.sha256"


def load_store() -> PointInTimeStore:
    return PointInTimeStore.from_csv(str(DATA))


def default_config() -> EngineConfig:
    return EngineConfig()


def report_verdict(reads: int = 0) -> str:
    return f"PASSED - masked replay reproduced every decision; {reads} future reads"
