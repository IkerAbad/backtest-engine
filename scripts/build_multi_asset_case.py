"""Build the multi-asset SPY/TLT case study report.

Only committed inputs are used: the two-asset CSV, its metadata and the engine.
The report has no generation timestamp or host details, so the same commit
produces the same bytes on every supported platform.

Usage:
    python scripts/build_multi_asset_case.py
"""

from __future__ import annotations

import hashlib
import json
import math
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import backtest_engine
from backtest_engine import (
    EngineConfig,
    MultiAssetFutureTrap,
    PointInTimeStore,
    RelativeMomentum,
    check_lookahead,
    walk_forward,
)
from backtest_engine.walkforward import Fold, WalkForwardResult

DATA = Path("data/spy_tlt_daily.csv")
META = Path("data/spy_tlt_daily.meta.json")
OUTPUT = Path("docs/multi_asset.html")
TRAIN_BARS = 504
TEST_BARS = 252
GRID = (63, 126, 252)
SYMBOLS = ("SPY", "TLT")
INITIAL_CASH = 1_000_000.0


def esc(value: object) -> str:
    text = str(value)
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&#39;")
    )


def date(ts: int) -> str:
    return datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%d")


def pct(value: float) -> str:
    return f"{value * 100:.4f}%"


def money(value: float) -> str:
    return f"{value:,.2f}"


def num(value: float, digits: int = 4) -> str:
    return f"{value:.{digits}f}"


def fold_dates(fold: Fold) -> tuple[str, str, str, str]:
    return (
        date(fold.train_start_ts),
        date(fold.train_end_ts - 1),
        date(fold.test_start_ts),
        date(fold.test_end_ts - 1),
    )


def curve_metrics(values: list[float]) -> dict[str, float]:
    returns: list[float] = []
    for index in range(1, len(values)):
        if values[index - 1] > 0.0:
            returns.append(values[index] / values[index - 1] - 1.0)
        else:
            returns.append(0.0)
    if len(returns) >= 2:
        mean = 0.0
        for value in returns:
            mean += value
        mean /= len(returns)
        squared = 0.0
        for value in returns:
            delta = value - mean
            squared += delta * delta
        variance = squared / (len(returns) - 1)
        stddev = math.sqrt(variance)
        volatility = math.sqrt(variance * 252.0)
        sharpe = mean / stddev * math.sqrt(252.0) if stddev > 0.0 else 0.0
    else:
        volatility = 0.0
        sharpe = 0.0
    peak = values[0] if values else 1.0
    max_drawdown = 0.0
    for value in values:
        if value > peak:
            peak = value
        drawdown = value / peak - 1.0 if peak > 0.0 else 0.0
        if drawdown < max_drawdown:
            max_drawdown = drawdown
    return {
        "total_return": values[-1] - 1.0 if values else 0.0,
        "volatility": volatility,
        "sharpe": sharpe,
        "max_drawdown": max_drawdown,
    }


def oos_series(walk: WalkForwardResult) -> tuple[list[tuple[int, float]], list[float]]:
    points: list[tuple[int, float]] = []
    returns: list[float] = []
    wealth = 1.0
    for fold_index, fold in enumerate(walk.folds):
        equity = fold.test.equity
        if fold_index == 0:
            points.append((equity[0].ts, 1.0))
        for index in range(1, len(equity)):
            previous = equity[index - 1].equity
            current = equity[index].equity
            value = current / previous - 1.0 if previous > 0.0 else 0.0
            returns.append(value)
            wealth *= 1.0 + value
            points.append((equity[index].ts, wealth))
    return points, returns


def fifty_fifty(
    store: PointInTimeStore, timestamps: list[int]
) -> tuple[list[tuple[int, float]], dict[str, float]]:
    closes: dict[str, list[float]] = {symbol: [] for symbol in SYMBOLS}
    for ts in timestamps:
        for symbol in SYMBOLS:
            bar = store.as_of(symbol, ts)
            if bar is None:
                raise RuntimeError(f"missing {symbol} bar at {date(ts)}")
            closes[symbol].append(bar.close)
    values = [
        0.5 * closes["SPY"][index] / closes["SPY"][0]
        + 0.5 * closes["TLT"][index] / closes["TLT"][0]
        for index in range(len(timestamps))
    ]
    return list(zip(timestamps, values, strict=True)), closes


def svg_chart(
    strategy_points: list[tuple[int, float]],
    benchmark_points: list[tuple[int, float]],
) -> str:
    width, height = 920.0, 340.0
    left, right, top, bottom = 72.0, 20.0, 18.0, 38.0
    plot_width = width - left - right
    plot_height = height - top - bottom
    values = [value for _, value in strategy_points] + [value for _, value in benchmark_points]
    minimum = min(values)
    maximum = max(values)
    span = maximum - minimum if maximum > minimum else 1.0

    def x(index: int, count: int) -> float:
        if count <= 1:
            return left + plot_width / 2.0
        return left + index / (count - 1) * plot_width

    def y(value: float) -> float:
        return top + (maximum - value) / span * plot_height

    strategy_path = " ".join(
        f"{x(index, len(strategy_points)):.2f},{y(value):.2f}"
        for index, (_, value) in enumerate(strategy_points)
    )
    benchmark_path = " ".join(
        f"{x(index, len(benchmark_points)):.2f},{y(value):.2f}"
        for index, (_, value) in enumerate(benchmark_points)
    )

    out = [
        f'<svg viewBox="0 0 {width:.0f} {height:.0f}" role="img" '
        'aria-label="Out-of-sample equity curve">',
        '<rect width="100%" height="100%" fill="#ffffff"/>',
    ]
    for step in range(3):
        value = minimum + span * step / 2.0
        py = y(value)
        out.append(
            f'<line x1="{left:.2f}" y1="{py:.2f}" x2="{width - right:.2f}" '
            f'y2="{py:.2f}" stroke="#e6e6e1" stroke-width="1"/>'
        )
        out.append(
            f'<text x="{left - 10.0:.2f}" y="{py + 4.0:.2f}" fill="#64676d" '
            f'font-size="11" text-anchor="end">{value:.2f}x</text>'
        )
    out.append(
        f'<polyline fill="none" stroke="#6b7280" stroke-width="1.6" points="{benchmark_path}"/>'
    )
    out.append(
        f'<polyline fill="none" stroke="#1d4ed8" stroke-width="2" '
        f'stroke-linejoin="round" points="{strategy_path}"/>'
    )
    out.append(
        f'<text x="{left:.2f}" y="{height - 12.0:.2f}" fill="#64676d" '
        f'font-size="11" text-anchor="start">{date(strategy_points[0][0])}</text>'
    )
    out.append(
        f'<text x="{width - right:.2f}" y="{height - 12.0:.2f}" fill="#64676d" '
        f'font-size="11" text-anchor="end">{date(strategy_points[-1][0])}</text>'
    )
    out.append(
        f'<rect x="{left:.2f}" y="{top + 6:.2f}" width="12" height="3" fill="#1d4ed8"/>'
        f'<text x="{left + 18:.2f}" y="{top + 12:.2f}" fill="#33363b" font-size="11">'
        "relative momentum</text>"
        f'<rect x="{left + 158:.2f}" y="{top + 6:.2f}" width="12" height="3" fill="#6b7280"/>'
        f'<text x="{left + 176:.2f}" y="{top + 12:.2f}" fill="#33363b" font-size="11">'
        "static 50/50</text>"
    )
    out.append("</svg>")
    return "\n".join(out)


def fold_rows(walk: WalkForwardResult) -> str:
    rows: list[str] = []
    for fold in walk.folds:
        train_start, train_end, test_start, test_end = fold_dates(fold)
        lookback = dict(fold.params)["lookback"]
        metrics = fold.test.metrics
        rows.append(
            "<tr>"
            f'<td class="num">{fold.index}</td>'
            f"<td>{train_start} to {train_end}</td>"
            f"<td>{test_start} to {test_end}</td>"
            f'<td class="num">{lookback}</td>'
            f'<td class="num">{num(fold.train.metrics.sharpe)}</td>'
            f'<td class="num">{pct(metrics.total_return)}</td>'
            f'<td class="num">{num(metrics.sharpe)}</td>'
            f'<td class="num">{pct(metrics.max_drawdown)}</td>'
            f'<td class="num">{metrics.n_trades}</td>'
            f'<td class="num">{money(metrics.total_cost)}</td>'
            "</tr>"
        )
    return "\n".join(rows)


def cost_rows(trades: list[Any]) -> tuple[str, dict[str, dict[str, float]]]:
    totals: dict[str, dict[str, float]] = {
        symbol: {
            "fills": 0.0,
            "notional": 0.0,
            "commission": 0.0,
            "spread": 0.0,
            "slippage": 0.0,
        }
        for symbol in SYMBOLS
    }
    for trade in trades:
        row = totals[trade.symbol]
        row["fills"] += 1.0
        row["notional"] += trade.notional
        row["commission"] += trade.commission
        row["spread"] += trade.spread_cost
        row["slippage"] += trade.slippage_cost
    rows: list[str] = []
    for symbol in SYMBOLS:
        row = totals[symbol]
        total = row["commission"] + row["spread"] + row["slippage"]
        rows.append(
            "<tr>"
            f"<td>{symbol}</td>"
            f'<td class="num">{int(row["fills"])}</td>'
            f'<td class="num">{money(row["notional"])}</td>'
            f'<td class="num">{money(row["commission"])}</td>'
            f'<td class="num">{money(row["spread"])}</td>'
            f'<td class="num">{money(row["slippage"])}</td>'
            f'<td class="num">{money(total)}</td>'
            "</tr>"
        )
    return "\n".join(rows), totals


def card(label: str, value: str, note: str) -> str:
    return (
        '<div class="card">'
        f'<div class="label">{esc(label)}</div>'
        f'<div class="value">{esc(value)}</div>'
        f'<div class="note">{esc(note)}</div>'
        "</div>"
    )


def build() -> str:
    meta: dict[str, Any] = json.loads(META.read_text(encoding="utf-8"))
    csv_bytes = DATA.read_bytes()
    csv_hash = hashlib.sha256(csv_bytes).hexdigest()
    if csv_hash != meta["csv_sha256"]:
        raise SystemExit("data/spy_tlt_daily.csv does not match its metadata hash")

    store = PointInTimeStore.from_csv(str(DATA))
    config = EngineConfig()
    factory = lambda **params: RelativeMomentum(  # noqa: E731
        lookback=int(str(params["lookback"]))
    )
    walk = walk_forward(
        factory,
        store,
        config,
        train_bars=TRAIN_BARS,
        test_bars=TEST_BARS,
        step_bars=TEST_BARS,
        grid=[{"lookback": lookback} for lookback in GRID],
    )

    strategy_points, _returns = oos_series(walk)
    timestamps = [ts for ts, _ in strategy_points]
    benchmark_points, _closes = fifty_fifty(store, timestamps)
    strategy_values = [value for _, value in strategy_points]
    benchmark_values = [value for _, value in benchmark_points]
    strategy_metrics = curve_metrics(strategy_values)
    benchmark_metrics = curve_metrics(benchmark_values)
    benchmark_return = benchmark_metrics["total_return"]

    honest = check_lookahead(
        lambda: RelativeMomentum(lookback=126),
        store,
        config,
        strategy_name="Relative momentum (126), full series",
    )
    if not honest.passed:
        raise SystemExit("honest relative momentum failed the look-ahead check")

    trap = check_lookahead(
        lambda: MultiAssetFutureTrap(),
        store,
        config,
        strategy_name="Multi-asset future trap",
    )
    if trap.passed:
        raise SystemExit("the multi-asset future trap was not detected")

    first_read = trap.future_accesses[0]
    first_divergence = trap.divergences[0]
    divergent_count = len(trap.divergences)
    normal_orders = ", ".join(
        f"{symbol} {weight:+.6f}" for symbol, weight in first_divergence.normal
    )
    masked_orders = ", ".join(
        f"{symbol} {weight:+.6f}" for symbol, weight in first_divergence.masked
    )

    all_trades = [trade for fold in walk.folds for trade in fold.test.trades]
    cost_table, cost_totals = cost_rows(all_trades)
    total_notional = sum(trade.notional for trade in all_trades)
    all_equity = [point.equity for fold in walk.folds for point in fold.test.equity]
    mean_equity = sum(all_equity) / len(all_equity) if all_equity else INITIAL_CASH
    turnover = total_notional / mean_equity if mean_equity > 0.0 else 0.0
    cost_bps = walk.total_cost / INITIAL_CASH * 10_000.0
    events = sum(fold.test.bars_processed for fold in walk.folds)
    timestamps_processed = sum(fold.test.metrics.n_bars for fold in walk.folds)
    selections = {"SPY": 0, "TLT": 0}
    for fold in walk.folds:
        for decision in fold.test.decisions:
            for order in decision.orders:
                if order.target_weight == 1.0:
                    selections[order.symbol] += 1
    oos_start = date(walk.folds[0].test_start_ts)
    oos_end = date(walk.folds[-1].test_end_ts - 1)
    comparison = "underperforms" if walk.total_return < benchmark_return else "outperforms"
    title = "Case study: SPY/TLT relative momentum walk-forward"

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(title)}</title>
<style>
:root{{--bg:#f7f7f5;--ink:#17181a;--muted:#64676d;--line:#dcdcd6;--card:#ffffff;--accent:#1d4ed8;--pass:#0f7a3d;--fail:#b42318;}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--ink);font:15px/1.55 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif}}
main{{max-width:1000px;margin:0 auto;padding:48px 24px 64px}}
.eyebrow{{letter-spacing:.14em;text-transform:uppercase;font-size:11px;color:var(--muted);margin:0 0 6px}}
h1{{font-size:30px;line-height:1.15;margin:0 0 8px;font-weight:650}}h2{{font-size:18px;margin:0 0 14px}}
.sub{{color:var(--muted);margin:0 0 26px}}
section{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:22px 24px;margin:18px 0}}
.verdict{{border-left:4px solid var(--accent);padding:14px 18px;margin:18px 0;background:#eef2ff}}
.verdict strong{{display:block;font-size:12px;letter-spacing:.1em;text-transform:uppercase;margin-bottom:4px;color:var(--accent)}}
.cards{{display:grid;grid-template-columns:repeat(3,1fr);gap:12px}}@media(max-width:720px){{.cards{{grid-template-columns:1fr 1fr}}}}
.card{{border:1px solid var(--line);border-radius:8px;padding:14px 16px;background:#fbfbfa}}.card .label{{font-size:12px;color:var(--muted);text-transform:uppercase;letter-spacing:.06em}}.card .value{{font-size:24px;font-variant-numeric:tabular-nums;margin-top:2px}}.card .note{{font-size:12px;color:var(--muted)}}
table{{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}}th,td{{text-align:left;padding:8px 10px;border-bottom:1px solid var(--line);vertical-align:top}}th{{font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);font-weight:600}}td.num,th.num{{text-align:right}}tr:last-child td{{border-bottom:0}}
code{{font:13px/1.4 ui-monospace,SFMono-Regular,Consolas,monospace;background:#f1f1ee;padding:2px 5px;border-radius:4px;word-break:break-all}}
.muted{{color:var(--muted)}}.scroll{{overflow-x:auto}}svg{{display:block;width:100%;height:auto}}
.pass{{border-left:4px solid var(--pass);background:#eff8f2;padding:12px 16px;border-radius:6px}}.fail{{border-left:4px solid var(--fail);background:#fdf1f0;padding:12px 16px;border-radius:6px}}
.pass strong{{color:var(--pass)}}.fail strong{{color:var(--fail)}}
footer{{color:var(--muted);font-size:12px;margin-top:28px;border-top:1px solid var(--line);padding-top:14px}}
</style>
</head>
<body>
<main>
<p class="eyebrow">backtest-engine &middot; multi-asset case study</p>
<h1>{esc(title)}</h1>
<p class="sub">SPY and TLT daily &middot; {date(store.first_ts or 0)} to {date(store.last_ts or 0)} &middot; {store.bar_count} bar events (2,514 per asset) &middot; adjusted for distributions</p>

<div class="verdict"><strong>Out-of-sample result</strong>
The two-asset strategy returned {pct(walk.total_return)} over {oos_start} to {oos_end},
with a Sharpe ratio of {num(walk.sharpe)} and a maximum drawdown of {pct(walk.max_drawdown)}.
The static 50/50 SPY/TLT reference returned {pct(benchmark_return)}. The strategy
{comparison} that reference.
</div>

<section><h2>What is measured</h2>
<p>This uses real daily histories for two assets. On the first bar of each month the strategy
computes each asset's return over the selected lookback and puts 100% of equity in the winner,
0% in the loser. The lookback is selected on each training block and applied to the following,
non-overlapping test block. The first test callback emits both legs; the second callback at the
same timestamp is a no-op.</p>
<p>The engine values the two assets independently, marks the portfolio at their latest closes and
charges every fill separately. The reference is a static 50/50 SPY/TLT portfolio, bought once at
the start of the out-of-sample period and never rebalanced, with no costs.</p>
</section>

<section><h2>Data provenance</h2><div class="scroll"><table><tbody>
<tr><th>Source</th><td><a href="{esc(meta["source_urls"]["SPY"])}">Yahoo Finance chart API</a></td></tr>
<tr><th>Retrieved</th><td>{esc(meta["retrieved_at"])}</td></tr>
<tr><th>Symbols / bars</th><td>SPY {meta["bars"]["SPY"]} &middot; TLT {meta["bars"]["TLT"]}</td></tr>
<tr><th>Range</th><td>{esc(meta["first_date"])} to {esc(meta["last_date"])}</td></tr>
<tr><th>CSV SHA-256</th><td><code>{csv_hash}</code></td></tr>
<tr><th>Engine data hash</th><td><code>{esc(store.data_hash)}</code></td></tr>
<tr><th>Adjustment</th><td>{esc(meta["adjustment"])}</td></tr>
</tbody></table></div></section>

<section><h2>Walk-forward design</h2>
<p>Training window: {TRAIN_BARS} timestamps. Test window: {TEST_BARS} timestamps. Step:
{TEST_BARS}, so test windows are contiguous and never overlap. Lookback grid:
{", ".join(str(lookback) for lookback in GRID)}. The winner is chosen by training Sharpe, then
total return, then grid order. {len(walk.folds)} folds fit in the available history. Across the
test folds the strategy selected SPY {selections["SPY"]} times and TLT {selections["TLT"]} times.</p>
</section>

<section><h2>Out-of-sample performance</h2><div class="cards">
{card("Total return", pct(walk.total_return), "compounded test folds")}
{card("Annualized volatility", pct(walk.annualized_volatility), "daily returns, 252 periods")}
{card("Sharpe ratio", num(walk.sharpe), "risk-free = 0")}
{card("Max drawdown", pct(walk.max_drawdown), "peak to trough")}
{card("Turnover", f"{turnover:.4f}x", "traded notional / mean equity")}
{card("Total cost", f"{money(walk.total_cost)}", f"{cost_bps:.2f} bps of initial capital")}
{card("Fills", str(walk.n_trades), "each asset fill counted separately")}
{card("Bar events", str(events), f"{timestamps_processed} timestamps x 2 assets")}
{card("Multi-asset", "SPY + TLT", "one target weight per asset")}
</div></section>

<section><h2>Out-of-sample equity</h2>
{svg_chart(strategy_points, benchmark_points)}
<p class="muted">The grey line is the static 50/50 SPY/TLT reference over the same test timestamps.
It is not rebalanced and has no costs.</p></section>

<section><h2>Strategy vs static 50/50</h2><div class="scroll"><table>
<thead><tr><th>Metric</th><th class="num">Relative momentum</th><th class="num">Static 50/50</th></tr></thead>
<tbody>
<tr><td>Total return</td><td class="num">{pct(strategy_metrics["total_return"])}</td><td class="num">{pct(benchmark_metrics["total_return"])}</td></tr>
<tr><td>Annualized volatility</td><td class="num">{pct(strategy_metrics["volatility"])}</td><td class="num">{pct(benchmark_metrics["volatility"])}</td></tr>
<tr><td>Sharpe ratio</td><td class="num">{num(strategy_metrics["sharpe"])}</td><td class="num">{num(benchmark_metrics["sharpe"])}</td></tr>
<tr><td>Max drawdown</td><td class="num">{pct(strategy_metrics["max_drawdown"])}</td><td class="num">{pct(benchmark_metrics["max_drawdown"])}</td></tr>
</tbody></table></div>
<p class="muted">The strategy {comparison} the static 50/50 reference on total return. The result is
published as measured, including the drawdown and cost drag.</p></section>

<section><h2>Capital allocation and per-asset costs</h2>
<p>Target weights are fractions of total portfolio equity. The engine sizes each order as
<code>(w_i * E - position_i * price_i) / price_i</code> at the decision close and fills it at that
asset's next open. The strategy uses <code>w_SPY + w_TLT = 1</code>, so gross exposure is 100% and
cash is the residual after fills. The engine does not enforce a budget; the strategy is responsible
for keeping the sum of absolute weights at or below 1.</p>
<p>Every fill is priced and charged on its own asset. A switch from SPY to TLT produces a sell fill
and a buy fill, so the fixed commission is paid twice. Commission is
<code>fixed + bps * |qty_i| * open_i / 10_000</code>; spread and slippage use that asset's open and
volume. The table below is the actual out-of-sample cost split.</p>
<div class="scroll"><table>
<thead><tr><th>Asset</th><th class="num">Fills</th><th class="num">Notional</th><th class="num">Commission</th><th class="num">Spread</th><th class="num">Slippage</th><th class="num">Total</th></tr></thead>
<tbody>
{cost_table}
<tr><td><strong>Total</strong></td><td class="num"><strong>{walk.n_trades}</strong></td><td class="num"><strong>{money(total_notional)}</strong></td><td class="num"><strong>{money(cost_totals["SPY"]["commission"] + cost_totals["TLT"]["commission"])}</strong></td><td class="num"><strong>{money(cost_totals["SPY"]["spread"] + cost_totals["TLT"]["spread"])}</strong></td><td class="num"><strong>{money(cost_totals["SPY"]["slippage"] + cost_totals["TLT"]["slippage"])}</strong></td><td class="num"><strong>{money(walk.total_cost)}</strong></td></tr>
</tbody></table></div></section>

<section><h2>Folds</h2><div class="scroll"><table>
<thead><tr><th class="num">Fold</th><th>Train</th><th>Test</th><th class="num">Lookback</th><th class="num">Train Sharpe</th><th class="num">Test return</th><th class="num">Test Sharpe</th><th class="num">Test max DD</th><th class="num">Fills</th><th class="num">Cost</th></tr></thead>
<tbody>
{fold_rows(walk)}
</tbody></table></div></section>

<section><h2>Look-ahead checks on the two-asset data</h2>
<div class="pass"><strong>Honest control: PASSED</strong>
Relative momentum (126) over the full series: {len(honest.future_accesses)} future reads,
{len(honest.divergences)} divergent decisions. The masked replay reproduced every decision.
</div>
<div class="fail" style="margin-top:12px"><strong>Multi-asset trap: FAILED, as required</strong>
The trap peeks at both assets' next bar on every rebalance: {len(trap.future_accesses)} future
reads, first kind <code>{esc(first_read.kind)}</code> on {first_read.clock_date}, requesting
{first_read.requested_date}. The first divergent decision is #{first_divergence.index} on
{first_divergence.date}: normal {normal_orders}; masked {masked_orders}. Divergent decisions:
{divergent_count}. Normal final equity {money(trap.normal.final_equity)}; masked final equity
{money(trap.masked.final_equity)}.
</div>
<p style="margin-top:14px">The trap is cross-sectional: it does not use a single future price, it
ranks two assets with their next-bar returns. The detector compares the per-symbol target weights
and finds the first month where the ranking changes. In the masked replay every future request
returns <code>None</code>, so the strategy stays flat. Strict mode rejects the future reads before a
raw report can be produced.</p></section>

<section><h2>Reproduce</h2>
<pre class="muted">python -m pip install .
python scripts/build_multi_asset_case.py
python scripts/verify_multi_asset_case.py</pre>
<p class="muted">The committed report hash is in <code>docs/multi_asset.sha256</code>. The numbers
above come from this exact CSV and this exact code. No result is hand-entered.</p></section>

<footer>Generated by backtest-engine {backtest_engine.__version__} &middot; deterministic report, no generation timestamp or host details &middot; MIT &middot; Iker Abad</footer>
</main>
</body>
</html>
"""
    return html


def main() -> int:
    html = build()
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_bytes(html.encode("utf-8"))
    digest = hashlib.sha256(html.encode("utf-8")).hexdigest()
    print(f"wrote {OUTPUT} ({len(html)} chars)")
    print(f"sha256: {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
