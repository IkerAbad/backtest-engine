"""Download a real daily history and prepare it for the point-in-time engine.

Source: Yahoo Finance chart API (no key, free for research use). Prices are
total-return adjusted with Yahoo's ``adjclose``: each OHLC value is multiplied
by ``adjclose / close``; volume is left untouched. The committed CSV is the
canonical dataset; rerunning this script may fetch a newer revision of history.

Usage:
    python scripts/fetch_real_data.py
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import time
import urllib.error
import urllib.request
from datetime import date, datetime, timezone
from datetime import time as dt_time
from pathlib import Path

HEADER = ["symbol", "ts", "published_at", "version", "open", "high", "low", "close", "volume"]
USER_AGENT = "Mozilla/5.0 (backtest-engine research script)"


def epoch(day: date) -> int:
    return int(datetime.combine(day, dt_time.min, tzinfo=timezone.utc).timestamp())


def fetch(symbol: str, start: date, end: date) -> dict[str, object]:
    url = (
        "https://query1.finance.yahoo.com/v8/finance/chart/"
        f"{symbol}?period1={epoch(start)}&period2={epoch(end)}"
        "&interval=1d&events=history&includeAdjustedClose=true"
    )
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    last_error: Exception | None = None
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                payload = json.loads(response.read().decode("utf-8"))
            chart = payload.get("chart", {})
            if chart.get("error"):
                raise RuntimeError(f"Yahoo error: {chart['error']}")
            results = chart.get("result") or []
            if not results:
                raise RuntimeError("Yahoo returned no result")
            return results[0]  # type: ignore[no-any-return]
        except (urllib.error.URLError, TimeoutError, RuntimeError) as error:
            last_error = error
            time.sleep(2 + attempt * 2)
    raise RuntimeError(f"download failed after retries: {last_error}")


def build_rows(result: dict[str, object], symbol: str) -> list[dict[str, object]]:
    timestamps = result["timestamp"]
    quote = result["indicators"]["quote"][0]  # type: ignore[index]
    adjusted = result["indicators"].get("adjclose", [{}])[0].get("adjclose", [])  # type: ignore[index]
    rows: list[dict[str, object]] = []

    for index, stamp in enumerate(timestamps):  # type: ignore[union-attr]
        raw_open = quote["open"][index]
        raw_high = quote["high"][index]
        raw_low = quote["low"][index]
        raw_close = quote["close"][index]
        volume = quote["volume"][index]
        if None in (raw_open, raw_high, raw_low, raw_close, volume):
            continue
        if volume <= 0 or raw_close <= 0:
            continue

        factor = 1.0
        if adjusted and index < len(adjusted) and adjusted[index] is not None:
            factor = float(adjusted[index]) / float(raw_close)

        open_price = round(float(raw_open) * factor, 6)
        close_price = round(float(raw_close) * factor, 6)
        high = round(float(raw_high) * factor, 6)
        low = round(float(raw_low) * factor, 6)
        high = max(high, open_price, close_price)
        low = min(low, open_price, close_price)
        if min(open_price, high, low, close_price) <= 0:
            continue

        day = datetime.fromtimestamp(int(stamp), tz=timezone.utc).date()
        close_time = datetime.combine(day, dt_time(21, 0), tzinfo=timezone.utc)
        iso = close_time.strftime("%Y-%m-%dT%H:%M:%SZ")
        rows.append(
            {
                "symbol": symbol,
                "ts": iso,
                "published_at": iso,
                "version": 1,
                "open": f"{open_price:.6f}",
                "high": f"{high:.6f}",
                "low": f"{low:.6f}",
                "close": f"{close_price:.6f}",
                "volume": int(volume),
            }
        )
    rows.sort(key=lambda row: str(row["ts"]))
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--symbol", default="SPY")
    parser.add_argument("--start", default="2016-11-21")
    parser.add_argument("--end", default="2026-11-22")
    parser.add_argument("--csv", default="data/spy_daily.csv")
    parser.add_argument("--meta", default="data/spy_daily.meta.json")
    parser.add_argument("--hash", default="data/spy_daily.sha256")
    args = parser.parse_args()

    start = date.fromisoformat(args.start)
    end = date.fromisoformat(args.end)
    result = fetch(args.symbol, start, end)
    rows = build_rows(result, args.symbol)
    if not rows:
        raise SystemExit("no usable bars returned")

    csv_path = Path(args.csv)
    csv_path.parent.mkdir(parents=True, exist_ok=True)
    with csv_path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=HEADER, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    raw = csv_path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()

    meta = {
        "symbol": args.symbol,
        "source": "Yahoo Finance chart API",
        "source_url": (
            "https://query1.finance.yahoo.com/v8/finance/chart/"
            f"{args.symbol}?period1={epoch(start)}&period2={epoch(end)}"
            "&interval=1d&events=history&includeAdjustedClose=true"
        ),
        "retrieved_at": datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "requested_start": args.start,
        "requested_end": args.end,
        "first_date": rows[0]["ts"],
        "last_date": rows[-1]["ts"],
        "bars": len(rows),
        "csv_sha256": digest,
        "adjustment": "OHLC scaled by Yahoo adjclose/close; volume unchanged",
        "notes": (
            "Daily bars timestamped at 21:00 UTC. Yahoo data is used for research; "
            "check the provider terms before redistributing or using commercially."
        ),
    }
    Path(args.meta).write_text(
        json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
    )
    Path(args.hash).write_text(f"{digest}  {csv_path.as_posix()}\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(rows)} bars to {csv_path}")
    print(f"range: {rows[0]['ts']} .. {rows[-1]['ts']}")
    print(f"sha256: {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
