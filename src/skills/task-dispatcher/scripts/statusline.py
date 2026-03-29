#!/usr/bin/env python3

from __future__ import annotations

import json
import sys
import time
from pathlib import Path
from typing import Any, Optional

STATUS_PATH = Path("__CCSWITCH_APP_CONFIG_DIR__") / "dispatch-status.json"


def read_status() -> Optional[dict[str, Any]]:
    if not STATUS_PATH.exists():
        return None
    try:
        payload = json.loads(STATUS_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return payload if isinstance(payload, dict) else None


def short_duration_from_ms(value: Any) -> str:
    if not isinstance(value, (int, float)):
        return "?s"
    seconds = float(value) / 1000.0
    if seconds >= 10:
        return "{:.0f}s".format(seconds)
    return "{:.1f}s".format(seconds)


def short_duration_from_seconds(value: Any) -> str:
    if not isinstance(value, (int, float)):
        return "?s"
    seconds = max(0.0, time.time() - float(value))
    if seconds >= 10:
        return "{:.0f}s".format(seconds)
    return "{:.1f}s".format(seconds)


def main() -> int:
    try:
        _ = sys.stdin.read()
    except Exception:
        pass

    snapshot = read_status()
    if not snapshot:
        print("dispatch: idle")
        return 0

    state = str(snapshot.get("state") or "idle")
    if state == "running":
        running_runs = snapshot.get("runningRuns")
        if isinstance(running_runs, list) and running_runs:
            current = running_runs[0] if isinstance(running_runs[0], dict) else None
            if isinstance(current, dict):
                target = str(current.get("target") or "unknown")
                elapsed = short_duration_from_seconds(current.get("startedAt"))
                extra = ""
                if len(running_runs) > 1:
                    extra = " +{}".format(len(running_runs) - 1)
                print("dispatch: running {}{} {}".format(target, extra, elapsed))
                return 0
        current = snapshot.get("currentRun")
        if isinstance(current, dict):
            target = str(current.get("target") or "unknown")
            elapsed = short_duration_from_seconds(current.get("startedAt"))
            print("dispatch: running {} {}".format(target, elapsed))
            return 0
        print("dispatch: running")
        return 0

    last_run = snapshot.get("lastRun")
    if isinstance(last_run, dict):
        target = str(last_run.get("target") or "unknown")
        status = str(last_run.get("status") or "idle")
        duration = short_duration_from_ms(last_run.get("durationMs"))
        if status == "succeeded":
            prefix = "ok"
        elif status == "timed_out":
            prefix = "timeout"
        elif status == "cancelled" or bool(last_run.get("cancelled")):
            prefix = "cancel"
        else:
            prefix = "fail"
        print("dispatch: {} {} {}".format(prefix, target, duration))
        return 0

    print("dispatch: idle")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
