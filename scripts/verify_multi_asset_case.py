"""Verify the committed multi-asset case study hash."""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

EXPECTED = Path("docs/multi_asset.sha256")
REPORT = Path("docs/multi_asset.html")


def main() -> int:
    if not EXPECTED.exists():
        print(f"missing {EXPECTED}", file=sys.stderr)
        return 1
    fields = EXPECTED.read_text(encoding="utf-8").split()
    if not fields:
        print(f"{EXPECTED} is empty", file=sys.stderr)
        return 1
    expected = fields[0]
    report = Path(fields[1]) if len(fields) > 1 else REPORT
    if not report.exists():
        print(f"missing report {report}", file=sys.stderr)
        return 1
    data = report.read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        print(
            f"multi-asset case study hash mismatch for {report}\n"
            f"expected: {expected}\n"
            f"actual:   {actual}\n"
            f"bytes:    {len(data)}",
            file=sys.stderr,
        )
        return 1
    print(f"multi-asset case study hash OK: {report} {actual}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
