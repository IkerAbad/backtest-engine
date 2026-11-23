"""Report writing.

The HTML is rendered by Rust. It is written as raw UTF-8 bytes so Windows
never turns ``\\n`` into ``\\r\\n``; that keeps the SHA-256 identical across
operating systems.
"""

from __future__ import annotations

from pathlib import Path

from ._backtest_engine import BacktestResult, sha256_hex


def write_report(
    result: BacktestResult,
    path: str | Path,
    *,
    strategy_name: str,
    title: str,
    lookahead_verdict: str,
) -> str:
    """Writes the report and returns its SHA-256."""
    html = result.render_html(strategy_name, title, lookahead_verdict)
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(html.encode("utf-8"))
    return sha256_hex(html)
