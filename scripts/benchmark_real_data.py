"""Measure one 10-year daily backtest.

The timing excludes CSV parsing and interpreter startup. It includes the full
engine run: event queue, point-in-time reads, strategy callbacks, order sizing,
fills, metrics and result construction. Peak RSS is for the whole process,
including the interpreter and the loaded dataset.

Usage:
    python scripts/benchmark_real_data.py
"""

from __future__ import annotations

import argparse
import ctypes
import json
import platform
import statistics
import subprocess
import sys
import time
from datetime import date
from pathlib import Path

from backtest_engine import EngineConfig, MovingAverage, PointInTimeStore, run_backtest

DATA = Path("data/spy_daily.csv")
ITERATIONS = 50
WARMUP = 5
WINDOW = 100


def cpu_name() -> str:
    if sys.platform == "win32":
        try:
            import winreg

            with winreg.OpenKey(
                winreg.HKEY_LOCAL_MACHINE,
                r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            ) as key:
                return str(winreg.QueryValueEx(key, "ProcessorNameString")[0]).strip()
        except OSError:
            pass
    if sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
        except OSError:
            pass
    if sys.platform == "darwin":
        try:
            return subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
            ).strip()
        except (OSError, subprocess.SubprocessError):
            pass
    return platform.processor() or platform.machine()


def rust_version() -> str:
    try:
        output = subprocess.check_output(["rustc", "--version"], text=True)
        return output.split()[1]
    except (OSError, subprocess.SubprocessError, IndexError):
        return "unknown"


def peak_rss_bytes() -> int:
    """Best-effort peak resident set size of this process."""
    if sys.platform == "win32":

        class ProcessMemoryCounters(ctypes.Structure):
            _fields_ = [
                ("cb", ctypes.c_ulong),
                ("PageFaultCount", ctypes.c_ulong),
                ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t),
            ]

        counters = ProcessMemoryCounters()
        counters.cb = ctypes.sizeof(ProcessMemoryCounters)
        psapi = ctypes.WinDLL("psapi.dll")
        kernel32 = ctypes.WinDLL("kernel32.dll")
        kernel32.GetCurrentProcess.restype = ctypes.c_void_p
        psapi.GetProcessMemoryInfo.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(ProcessMemoryCounters),
            ctypes.c_ulong,
        ]
        psapi.GetProcessMemoryInfo.restype = ctypes.c_int
        if psapi.GetProcessMemoryInfo(
            kernel32.GetCurrentProcess(), ctypes.byref(counters), counters.cb
        ):
            return int(counters.PeakWorkingSetSize)
        raise OSError(f"GetProcessMemoryInfo failed: {ctypes.get_last_error()}")
    elif sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/self/status").read_text(encoding="utf-8").splitlines():
                if line.startswith("VmHWM:"):
                    return int(line.split()[1]) * 1024
        except OSError:
            pass
    elif sys.platform == "darwin":
        import resource

        return int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default=None, help="optional JSON output path")
    args = parser.parse_args()

    store = PointInTimeStore.from_csv(str(DATA))
    config = EngineConfig()
    factory = lambda: MovingAverage(window=WINDOW)  # noqa: E731

    for _ in range(WARMUP):
        run_backtest(factory(), store, config)

    times: list[float] = []
    last = None
    for _ in range(ITERATIONS):
        start = time.perf_counter()
        last = run_backtest(factory(), store, config)
        times.append(time.perf_counter() - start)

    assert last is not None
    median = statistics.median(times)
    minimum = min(times)
    maximum = max(times)
    events = last.bars_processed
    rss = peak_rss_bytes()
    result = {
        "date": date.today().isoformat(),
        "hardware": cpu_name(),
        "os": platform.platform(),
        "python": platform.python_version(),
        "python_implementation": platform.python_implementation(),
        "rust_toolchain": rust_version(),
        "dataset": DATA.as_posix(),
        "bars": events,
        "strategy": f"MovingAverage(window={WINDOW})",
        "iterations": ITERATIONS,
        "warmup": WARMUP,
        "median_ms": median * 1000.0,
        "min_ms": minimum * 1000.0,
        "max_ms": maximum * 1000.0,
        "events_per_second": events / median,
        "peak_rss_mb": rss / (1024.0 * 1024.0),
        "peak_rss_method": (
            "GetProcessMemoryInfo PeakWorkingSetSize"
            if sys.platform == "win32"
            else "VmHWM"
            if sys.platform.startswith("linux")
            else "ru_maxrss"
            if sys.platform == "darwin"
            else "unavailable"
        ),
        "total_return": last.metrics.total_return,
        "sharpe": last.metrics.sharpe,
    }
    rendered = json.dumps(result, indent=2) + "\n"
    if args.out is not None:
        Path(args.out).write_text(rendered, encoding="utf-8", newline="\n")
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
