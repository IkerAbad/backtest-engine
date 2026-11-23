"""Verify the committed report hash.

The report job runs this on every supported OS. A match proves that the
engine produced byte-identical HTML on different machines.
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

EXPECTED = Path("reports/expected.sha256")
DEFAULT_REPORT = Path("reports/example.html")


def main() -> int:
    if not EXPECTED.exists():
        print(f"missing {EXPECTED}", file=sys.stderr)
        return 1
    fields = EXPECTED.read_text(encoding="utf-8").split()
    if not fields:
        print(f"{EXPECTED} is empty", file=sys.stderr)
        return 1
    expected = fields[0]
    report_path = Path(fields[1]) if len(fields) > 1 else DEFAULT_REPORT
    if not report_path.exists():
        print(f"missing report {report_path}", file=sys.stderr)
        return 1

    data = report_path.read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        print(
            f"report hash mismatch for {report_path}\n"
            f"expected: {expected}\n"
            f"actual:   {actual}\n"
            f"bytes:    {len(data)}",
            file=sys.stderr,
        )
        return 1
    print(f"report hash OK: {report_path} {actual}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
