#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Optional, Union

APP_CONFIG_DIR = Path("__CCSWITCH_APP_CONFIG_DIR__")
DISCOVERY_PATH = APP_CONFIG_DIR / "dispatch-api.json"
HISTORY_PATH = APP_CONFIG_DIR / "dispatch-history.jsonl"
STATUS_PATH = APP_CONFIG_DIR / "dispatch-status.json"
DEFAULT_TIMEOUT_SECONDS = 120
MAX_TIMEOUT_SECONDS = 900
DEFAULT_LOG_LIMIT = 10
MAX_LOG_LIMIT = 50
SUPPORTED_APPS = ("claude", "codex")
SUPPORTED_MONITORS = ("none", "pane")
FINISHED_STATUSES = {"succeeded", "failed", "timed_out", "cancelled"}
CALLBACK_TAG = "MAIN_AGENT_CALLBACK"
CALLBACK_BLOCK_RE = re.compile(
    rf"<<{CALLBACK_TAG}>>\s*"
    r"status:\s*(?P<status>[^\n\r]+)\s*\n"
    r"message:\s*(?P<message>[^\n\r]*)\s*\n"
    r"summary:\s*(?P<summary>[^\n\r]*)\s*\n"
    r"deliverable:\s*\n"
    rf"(?P<deliverable>.*?)\n<</{CALLBACK_TAG}>>",
    re.DOTALL,
)


class DispatchError(RuntimeError):
    pass


@dataclass
class ProvidersCommand:
    app: Optional[str]


@dataclass
class StatusCommand:
    pass


@dataclass
class LastCommand:
    pass


@dataclass
class LogsCommand:
    limit: int


@dataclass
class ListCommand:
    limit: int


@dataclass
class ShowCommand:
    run_id: str


@dataclass
class WatchCommand:
    run_id: str


@dataclass
class CancelCommand:
    run_id: str


@dataclass
class BridgeRunInternalCommand:
    spec_path: str


@dataclass
class RunCommand:
    target: str
    timeout_seconds: int
    wait_for_completion: bool
    monitor: str
    task: str


Command = Union[
    ProvidersCommand,
    StatusCommand,
    LastCommand,
    LogsCommand,
    ListCommand,
    ShowCommand,
    WatchCommand,
    CancelCommand,
    BridgeRunInternalCommand,
    RunCommand,
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Invoke the cc-switch dispatch loopback API.",
    )
    parser.add_argument(
        "--cwd",
        default=os.getcwd(),
        help="Working directory to send to the dispatch API.",
    )
    parser.add_argument(
        "--raw-arguments",
        help="Raw skill arguments. If omitted, the helper reads stdin.",
    )
    return parser.parse_args()


def load_raw_arguments(raw_arguments: Optional[str]) -> str:
    raw = raw_arguments if raw_arguments is not None else sys.stdin.read()
    raw = raw.strip()
    if not raw:
        raise DispatchError(usage_error("Missing dispatch arguments."))
    return raw


def usage_lines() -> list[str]:
    return [
        "- /dispatch-task providers [app]",
        "- /dispatch-task status",
        "- /dispatch-task last",
        "- /dispatch-task logs [count]",
        "- /dispatch-task list [count]",
        "- /dispatch-task show <run_id>",
        "- /dispatch-task watch <run_id>",
        "- /dispatch-task cancel <run_id>",
        "- /dispatch-task <app:provider> [timeout=<seconds>] [wait=true] [monitor=pane|none] -- <task text>",
    ]


def usage_error(message: str) -> str:
    return "{}\n\nUsage:\n{}".format(message, "\n".join(usage_lines()))


def parse_bool_flag(raw: str) -> bool:
    normalized = raw.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    raise DispatchError(
        "Invalid wait value '{}'. Expected true or false.".format(raw)
    )


def parse_limit_token(raw: str) -> int:
    try:
        limit = int(raw, 10)
    except ValueError as exc:
        raise DispatchError("Count must be an integer.") from exc
    if limit < 1 or limit > MAX_LOG_LIMIT:
        raise DispatchError(
            "Count must be between 1 and {}.".format(MAX_LOG_LIMIT)
        )
    return limit


def parse_command(raw: str) -> Command:
    tokens = shlex.split(raw)
    if not tokens:
        raise DispatchError(usage_error("Missing dispatch arguments."))

    head = tokens[0].strip().lower()
    if head == "providers":
        if len(tokens) > 2:
            raise DispatchError("`providers` accepts at most one optional app filter.")
        app = None
        if len(tokens) == 2:
            app = tokens[1].strip().lower()
            if app not in SUPPORTED_APPS:
                raise DispatchError(
                    "Unknown app filter '{}'. Expected one of: {}.".format(
                        tokens[1], ", ".join(SUPPORTED_APPS)
                    )
                )
        return ProvidersCommand(app=app)

    if head == "status":
        if len(tokens) != 1:
            raise DispatchError("`status` does not accept extra arguments.")
        return StatusCommand()

    if head == "last":
        if len(tokens) != 1:
            raise DispatchError("`last` does not accept extra arguments.")
        return LastCommand()

    if head == "logs":
        if len(tokens) > 2:
            raise DispatchError("`logs` accepts at most one optional numeric count.")
        return LogsCommand(
            limit=parse_limit_token(tokens[1]) if len(tokens) == 2 else DEFAULT_LOG_LIMIT
        )

    if head == "list":
        if len(tokens) > 2:
            raise DispatchError("`list` accepts at most one optional numeric count.")
        return ListCommand(
            limit=parse_limit_token(tokens[1]) if len(tokens) == 2 else DEFAULT_LOG_LIMIT
        )

    if head in {"show", "watch", "cancel"}:
        if len(tokens) != 2:
            raise DispatchError("`{}` requires exactly one run id.".format(head))
        run_id = tokens[1].strip()
        if not run_id:
            raise DispatchError("Run id cannot be empty.")
        if head == "show":
            return ShowCommand(run_id=run_id)
        if head == "watch":
            return WatchCommand(run_id=run_id)
        return CancelCommand(run_id=run_id)

    if head == "__bridge-run":
        if len(tokens) != 2:
            raise DispatchError("`__bridge-run` requires exactly one spec path.")
        spec_path = tokens[1].strip()
        if not spec_path:
            raise DispatchError("Bridge spec path cannot be empty.")
        return BridgeRunInternalCommand(spec_path=spec_path)

    match = re.match(r"(?s)^(?P<head>.+?)\s+--\s*(?P<task>.+)$", raw)
    if not match:
        raise DispatchError(usage_error("Missing `-- <task text>` separator."))

    run_head = match.group("head").strip()
    task = match.group("task").strip()
    if not task:
        raise DispatchError("Task text cannot be empty.")

    head_tokens = shlex.split(run_head)
    if not head_tokens:
        raise DispatchError("Missing dispatch target.")

    target = head_tokens[0].strip()
    app, provider_id = parse_target(target)

    timeout_seconds = DEFAULT_TIMEOUT_SECONDS
    wait_for_completion = False
    monitor = "none"
    for token in head_tokens[1:]:
        if token.startswith("timeout="):
            raw_timeout = token.split("=", 1)[1].strip()
            if not raw_timeout:
                raise DispatchError("timeout=<seconds> requires a numeric value.")
            try:
                timeout_seconds = int(raw_timeout, 10)
            except ValueError as exc:
                raise DispatchError(
                    "Invalid timeout '{}'. Expected an integer number of seconds.".format(
                        raw_timeout
                    )
                ) from exc
            if timeout_seconds < 1 or timeout_seconds > MAX_TIMEOUT_SECONDS:
                raise DispatchError(
                    "timeout must be between 1 and {} seconds.".format(
                        MAX_TIMEOUT_SECONDS
                    )
                )
            continue

        if token.startswith("wait="):
            raw_wait = token.split("=", 1)[1].strip()
            if not raw_wait:
                raise DispatchError("wait=<true|false> requires a value.")
            wait_for_completion = parse_bool_flag(raw_wait)
            continue

        if token.startswith("monitor="):
            monitor = token.split("=", 1)[1].strip().lower()
            if monitor not in SUPPORTED_MONITORS:
                raise DispatchError(
                    "Unsupported monitor '{}'. Expected one of: {}.".format(
                        monitor, ", ".join(SUPPORTED_MONITORS)
                    )
                )
            continue

        raise DispatchError(
            "Unexpected argument '{}'. Supported options: timeout=<seconds>, wait=<true|false>, monitor=pane|none.".format(
                token
            )
        )

    if wait_for_completion and monitor == "pane":
        raise DispatchError("monitor=pane cannot be combined with wait=true.")

    return RunCommand(
        target="{}:{}".format(app, provider_id),
        timeout_seconds=timeout_seconds,
        wait_for_completion=wait_for_completion,
        monitor=monitor,
        task=task,
    )


def parse_target(target: str) -> tuple[str, str]:
    app, separator, provider_selector = target.partition(":")
    app = app.strip().lower()
    provider_selector = provider_selector.strip()

    if separator != ":" or not provider_selector:
        raise DispatchError(
            "Target must use the form `claude:<provider>` or `codex:<provider>`."
        )
    if app not in SUPPORTED_APPS:
        raise DispatchError(
            "Unsupported target app '{}'. Expected one of: {}.".format(
                app, ", ".join(SUPPORTED_APPS)
            )
        )

    return app, provider_selector


def load_discovery() -> dict[str, Any]:
    if not DISCOVERY_PATH.exists():
        raise DispatchError(
            "cc-switch dispatch service was not found.\n\n"
            "Start the cc-switch desktop app, then try again."
        )

    try:
        payload = json.loads(DISCOVERY_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise DispatchError(
            "Failed to read {}: {}".format(DISCOVERY_PATH, exc)
        ) from exc
    except json.JSONDecodeError as exc:
        raise DispatchError(
            "Failed to parse {}: {}".format(DISCOVERY_PATH, exc)
        ) from exc

    if not payload.get("baseUrl") or not payload.get("token"):
        raise DispatchError(
            "{} is missing required fields. Restart cc-switch and try again.".format(
                DISCOVERY_PATH
            )
        )

    return payload


def request_json(
    discovery: dict[str, Any],
    method: str,
    path: str,
    payload: Optional[dict[str, Any]] = None,
    timeout_seconds: int = 30,
) -> dict[str, Any]:
    return request_json_with_credentials(
        str(discovery["baseUrl"]),
        str(discovery["token"]),
        method,
        path,
        payload=payload,
        timeout_seconds=timeout_seconds,
    )


def request_json_with_credentials(
    base_url: str,
    token: str,
    method: str,
    path: str,
    payload: Optional[dict[str, Any]] = None,
    timeout_seconds: int = 30,
) -> dict[str, Any]:
    base_url = base_url.rstrip("/")
    url = base_url + path
    data = None
    headers = {
        "Authorization": "Bearer {}".format(token),
        "Accept": "application/json",
    }

    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"

    request = urllib.request.Request(url, data=data, headers=headers, method=method)

    try:
        with urllib.request.urlopen(request, timeout=timeout_seconds) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        raise DispatchError(extract_api_error(body, exc.reason)) from exc
    except urllib.error.URLError as exc:
        raise DispatchError(
            "Unable to reach cc-switch dispatch service at {}.\n\n"
            "Make sure the desktop app is still running.".format(base_url)
        ) from exc

    try:
        return json.loads(body)
    except json.JSONDecodeError as exc:
        raise DispatchError(
            "Dispatch service returned invalid JSON: {}".format(exc)
        ) from exc


def load_bridge_spec(spec_path: str) -> dict[str, Any]:
    path = Path(spec_path).expanduser()
    if not path.exists():
        raise DispatchError("Bridge spec does not exist: {}".format(path))
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise DispatchError("Failed to read {}: {}".format(path, exc)) from exc
    except json.JSONDecodeError as exc:
        raise DispatchError("Failed to parse {}: {}".format(path, exc)) from exc
    if not isinstance(payload, dict):
        raise DispatchError("{} must contain a JSON object.".format(path))
    return payload


def extract_api_error(body: str, fallback: str) -> str:
    try:
        payload = json.loads(body)
    except json.JSONDecodeError:
        return body.strip() or str(fallback)

    if isinstance(payload, dict):
        error = payload.get("error")
        if isinstance(error, str) and error.strip():
            return error.strip()

    return body.strip() or str(fallback)


def load_status_snapshot() -> Optional[dict[str, Any]]:
    return load_json_file(STATUS_PATH)


def load_json_file(path: Path) -> Optional[dict[str, Any]]:
    if not path.exists():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise DispatchError("Failed to read {}: {}".format(path, exc)) from exc
    if not isinstance(payload, dict):
        raise DispatchError("{} must contain a JSON object.".format(path))
    return payload


def load_history_entries(limit: int) -> list[dict[str, Any]]:
    if not HISTORY_PATH.exists():
        return []
    try:
        lines = HISTORY_PATH.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise DispatchError("Failed to read {}: {}".format(HISTORY_PATH, exc)) from exc

    entries: list[dict[str, Any]] = []
    for line in reversed(lines):
        line = line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            entries.append(payload)
        if len(entries) >= limit:
            break
    return entries


def request_runs(
    discovery: dict[str, Any], limit: int, status: Optional[str] = None
) -> list[dict[str, Any]]:
    query = {"limit": str(limit)}
    if status:
        query["status"] = status
    path = "/v1/dispatch/runs?{}".format(urllib.parse.urlencode(query))
    payload = request_json(discovery, "GET", path)
    runs = payload.get("runs")
    if not isinstance(runs, list):
        raise DispatchError("Dispatch service returned an invalid runs payload.")
    return [item for item in runs if isinstance(item, dict)]


def request_run(discovery: dict[str, Any], run_id: str) -> dict[str, Any]:
    payload = request_json(discovery, "GET", "/v1/dispatch/runs/{}".format(run_id))
    if not isinstance(payload, dict):
        raise DispatchError("Dispatch service returned an invalid run payload.")
    return payload


def request_cancel(discovery: dict[str, Any], run_id: str) -> dict[str, Any]:
    payload = request_json(
        discovery, "POST", "/v1/dispatch/runs/{}/cancel".format(run_id), payload={}
    )
    if not isinstance(payload, dict):
        raise DispatchError("Dispatch service returned an invalid cancel payload.")
    return payload


def request_bridge_prepare(
    discovery: dict[str, Any], payload: dict[str, Any]
) -> dict[str, Any]:
    response = request_json(
        discovery,
        "POST",
        "/v1/dispatch/bridge",
        payload=payload,
        timeout_seconds=max(int(payload.get("timeoutSeconds") or 30), 30),
    )
    if not isinstance(response, dict):
        raise DispatchError("Dispatch service returned an invalid bridge payload.")
    return response


def request_bridge_started(spec: dict[str, Any], pane_id: str) -> dict[str, Any]:
    response = request_json_with_credentials(
        str(spec["baseUrl"]),
        str(spec["token"]),
        "POST",
        "/v1/dispatch/runs/{}/bridge-start".format(spec["runId"]),
        payload={"paneId": pane_id},
        timeout_seconds=15,
    )
    if not isinstance(response, dict):
        raise DispatchError("Dispatch service returned an invalid bridge-start payload.")
    return response


def request_bridge_completed(spec: dict[str, Any], payload: dict[str, Any]) -> dict[str, Any]:
    response = request_json_with_credentials(
        str(spec["baseUrl"]),
        str(spec["token"]),
        "POST",
        "/v1/dispatch/runs/{}/bridge-complete".format(spec["runId"]),
        payload=payload,
        timeout_seconds=30,
    )
    if not isinstance(response, dict):
        raise DispatchError("Dispatch service returned an invalid bridge-complete payload.")
    return response


def format_duration(duration_ms: Any) -> str:
    if not isinstance(duration_ms, (int, float)):
        return "unknown"

    seconds = float(duration_ms) / 1000.0
    if seconds >= 10:
        return "{:.1f}s".format(seconds)
    return "{:.2f}s".format(seconds)


def format_timestamp(timestamp: Any) -> str:
    if not isinstance(timestamp, (int, float)):
        return "unknown"
    return datetime.fromtimestamp(float(timestamp)).astimezone().strftime(
        "%Y-%m-%d %H:%M:%S %Z"
    )


def format_elapsed(started_at: Any) -> str:
    if not isinstance(started_at, (int, float)):
        return "unknown"
    elapsed = max(0, int(time.time() - float(started_at)))
    if elapsed < 60:
        return "{}s".format(elapsed)
    minutes, seconds = divmod(elapsed, 60)
    if minutes < 60:
        return "{}m {}s".format(minutes, seconds)
    hours, minutes = divmod(minutes, 60)
    return "{}h {}m".format(hours, minutes)


def parse_callback_block(text: str) -> Optional[dict[str, str]]:
    normalized = text.replace("\r\n", "\n")
    match = CALLBACK_BLOCK_RE.search(normalized)
    if not match:
        return None
    return {
        "status": match.group("status").strip(),
        "message": match.group("message").strip(),
        "summary": match.group("summary").strip(),
        "deliverable": match.group("deliverable").strip(),
    }


def extract_callback_block_text(text: str) -> Optional[str]:
    normalized = text.replace("\r\n", "\n")
    match = CALLBACK_BLOCK_RE.search(normalized)
    if not match:
        return None
    return match.group(0).strip()


def strip_callback_block(text: str) -> str:
    normalized = text.replace("\r\n", "\n")
    return CALLBACK_BLOCK_RE.sub("", normalized).strip()


def short_run_id(run_id: str) -> str:
    return run_id[:8] if run_id else "legacy"


def render_providers(payload: dict[str, Any], app_filter: Optional[str]) -> str:
    providers = payload.get("providers")
    if not isinstance(providers, list):
        raise DispatchError("Dispatch service returned an invalid providers payload.")

    title = "## Dispatch Providers"
    if app_filter:
        title += " (`{}`)".format(app_filter)

    lines = [title, ""]

    if not providers:
        lines.append("- No dispatchable providers found in cc-switch.")
        lines.append("- Supported apps in v1: `claude`, `codex`.")
    else:
        for item in providers:
            if not isinstance(item, dict):
                continue
            target = str(item.get("target") or "unknown")
            name = str(item.get("providerName") or item.get("providerId") or "unknown")
            suffix = " (current)" if item.get("current") else ""
            lines.append("- `{}` - {}{}".format(target, name, suffix))

    lines.extend(["", "Usage:"] + usage_lines())
    return "\n".join(lines)


def render_status_from_api(discovery: dict[str, Any]) -> str:
    runs = request_runs(discovery, limit=10)
    running = [
        run for run in runs if str(run.get("status") or "").strip() in {"queued", "running"}
    ]
    finished = [
        run for run in runs if str(run.get("status") or "").strip() in FINISHED_STATUSES
    ]

    lines = ["## Dispatch Status", ""]
    lines.append("- State: `{}`".format("running" if running else "idle"))
    if running:
        lines.append("- Active runs: `{}`".format(len(running)))
        for run in running[:3]:
            lines.append(
                "- `{}` | `{}` | `{}` | started `{}` | cwd=`{}`".format(
                    short_run_id(str(run.get("runId") or "")),
                    run.get("status") or "unknown",
                    run.get("target") or "unknown",
                    format_timestamp(run.get("startedAt")),
                    run.get("cwd") or "n/a",
                )
            )
    else:
        lines.append("- No runs are currently active.")

    if finished:
        lines.extend(["", "### Last Finished Run"])
        lines.extend(render_compact_last_run(finished[0]))

    return "\n".join(lines)


def render_status_fallback() -> str:
    snapshot = load_status_snapshot()
    history_entry = load_history_entries(1)
    last_entry = history_entry[0] if history_entry else None

    lines = ["## Dispatch Status", ""]
    if not snapshot and not last_entry:
        lines.append("- State: `idle`")
        lines.append("- No dispatch activity has been recorded yet.")
        return "\n".join(lines)

    if snapshot:
        state = str(snapshot.get("state") or "idle")
        updated_at = snapshot.get("updatedAt")
        lines.append("- State: `{}`".format(state))
        if updated_at is not None:
            lines.append("- Updated: `{}`".format(format_timestamp(updated_at)))
        current_run = snapshot.get("currentRun")
        if state == "running" and isinstance(current_run, dict):
            lines.append("- Run ID: `{}`".format(current_run.get("runId") or "unknown"))
            lines.append("- Target: `{}`".format(current_run.get("target") or "unknown"))
            lines.append("- Provider: `{}`".format(current_run.get("providerName") or "unknown"))
        last_run = snapshot.get("lastRun")
        if isinstance(last_run, dict):
            lines.extend(render_compact_last_run(last_run))
    elif last_entry:
        lines.append("- State: `idle`")
        lines.extend(render_compact_last_run(last_entry))

    return "\n".join(lines)


def render_compact_last_run(entry: dict[str, Any]) -> list[str]:
    callback = parse_callback_block(str(entry.get("result") or ""))
    callback_status = callback["status"] if callback else "missing"
    finished_at = entry.get("finishedAt")
    if finished_at is None:
        finished_at = entry.get("timestamp") or entry.get("updatedAt")
    return [
        "- Last run ID: `{}`".format(entry.get("runId") or "unknown"),
        "- Last target: `{}`".format(entry.get("target") or "unknown"),
        "- Last outcome: `{}`".format(entry.get("status") or "unknown"),
        "- Last callback: `{}`".format(callback_status),
        "- Last finished: `{}`".format(format_timestamp(finished_at)),
        "- Last duration: `{}`".format(format_duration(entry.get("durationMs"))),
    ]


def latest_finished_run(discovery: dict[str, Any]) -> Optional[dict[str, Any]]:
    runs = request_runs(discovery, limit=20)
    for run in runs:
        if str(run.get("status") or "") in FINISHED_STATUSES:
            return run
    return None


def render_last(discovery: dict[str, Any]) -> str:
    record = latest_finished_run(discovery)
    if not record:
        return "## Dispatch Last Run\n\n- No dispatch history is available yet."
    return render_record("Dispatch Last Run", record)


def render_logs(discovery: dict[str, Any], limit: int) -> str:
    entries = request_runs(discovery, limit=limit)
    lines = [
        "## Dispatch Logs",
        "",
        "- Showing the newest `{}` entr{}.".format(limit, "y" if limit == 1 else "ies"),
    ]

    if not entries:
        lines.append("- No dispatch history is available yet.")
        return "\n".join(lines)

    for entry in entries:
        callback = parse_callback_block(str(entry.get("result") or ""))
        callback_status = callback["status"] if callback else "missing"
        lines.append(
            "- `{}` | `{}` | `{}` | `{}` | callback=`{}` | exit=`{}`".format(
                format_timestamp(entry.get("finishedAt") or entry.get("updatedAt")),
                entry.get("status") or "unknown",
                entry.get("target") or "unknown",
                format_duration(entry.get("durationMs")),
                callback_status,
                entry.get("exitCode") if entry.get("exitCode") is not None else "n/a",
            )
        )
        preview = str(entry.get("resultPreview") or "").strip()
        if preview:
            lines.append("  preview: {}".format(preview.replace("\n", " ")))

    return "\n".join(lines)


def render_list(discovery: dict[str, Any], limit: int) -> str:
    entries = request_runs(discovery, limit=limit)
    lines = ["## Dispatch Runs", ""]
    if not entries:
        lines.append("- No dispatch runs are available yet.")
        return "\n".join(lines)

    for entry in entries:
        lines.append(
            "- `{}` | `{}` | `{}` | `{}` | `{}`".format(
                short_run_id(str(entry.get("runId") or "")),
                entry.get("status") or "unknown",
                entry.get("target") or "unknown",
                format_timestamp(entry.get("startedAt")),
                format_duration(entry.get("durationMs")),
            )
        )
        preview = str(entry.get("taskPreview") or entry.get("resultPreview") or "").strip()
        if preview:
            lines.append("  preview: {}".format(preview.replace("\n", " ")))

    return "\n".join(lines)


def render_record(title: str, record: dict[str, Any]) -> str:
    timed_out = bool(record.get("timedOut"))
    cancelled = bool(record.get("cancelled"))
    status = str(record.get("status") or "unknown")
    target = str(record.get("target") or "unknown")
    provider_name = str(record.get("providerName") or "unknown")
    exit_code = record.get("exitCode")
    duration = format_duration(record.get("durationMs"))
    cwd = str(record.get("cwd") or "")
    result = str(record.get("result") or "").strip()
    stdout = str(record.get("stdout") or "").strip()
    stderr = str(record.get("stderr") or "").strip()
    callback = parse_callback_block(result)

    outcome = status
    if timed_out:
        outcome = "timed_out"
    elif cancelled:
        outcome = "cancelled"

    lines = [
        "## {}".format(title),
        "",
        "- Run ID: `{}`".format(record.get("runId") or "unknown"),
        "- Outcome: `{}`".format(outcome),
        "- Target: `{}`".format(target),
        "- Provider: `{}`".format(provider_name),
        "- Duration: `{}`".format(duration),
        "- Exit code: `{}`".format(exit_code if exit_code is not None else "n/a"),
        "- Callback: `{}`".format(callback["status"] if callback else "missing"),
    ]

    finished_at = record.get("finishedAt")
    if finished_at is None:
        finished_at = record.get("timestamp") or record.get("updatedAt")
    if finished_at is not None:
        lines.append("- Finished: `{}`".format(format_timestamp(finished_at)))
    if cwd:
        lines.append("- CWD: `{}`".format(cwd))

    task_preview = str(record.get("taskPreview") or "").strip()
    if task_preview:
        lines.append("- Task: {}".format(task_preview))

    lines.extend(["", "### Main Agent Callback"])
    if callback:
        lines.append("- Message: {}".format(callback["message"] or "_empty_"))
        lines.append("- Summary: {}".format(callback["summary"] or "_empty_"))
        lines.extend(["", "### Deliverable", callback["deliverable"] or "_Empty deliverable._"])

        remainder = strip_callback_block(result)
        if remainder:
            lines.extend(["", "### Child Notes", remainder])
    else:
        lines.append("- Callback block was not found in the child result.")
        body = result or stdout or stderr or "No result captured."
        lines.extend(["", "### Child Result", body])

    if stderr and stderr != result:
        lines.extend(["", "### stderr", fenced_block(stderr)])
    elif stdout and stdout != result and outcome != "succeeded":
        lines.extend(["", "### stdout", fenced_block(stdout)])

    return "\n".join(lines)


def fenced_block(text: str) -> str:
    return "```text\n{}\n```".format(text)


def render_failure(message: str) -> str:
    return "## Dispatch Error\n\n- Error: {}".format(message.strip())


def render_started(
    record: dict[str, Any],
    pane_id: Optional[str] = None,
    pane_label: str = "Monitor pane",
) -> str:
    state = str(record.get("state") or "queued")
    lines = [
        "## Dispatch Started",
        "",
        "- State: `{}`".format(state),
        "- Run ID: `{}`".format(record.get("runId") or "unknown"),
        "- Target: `{}`".format(record.get("target") or "unknown"),
        "- Provider: `{}`".format(record.get("providerName") or "unknown"),
        "- Started: `{}`".format(format_timestamp(record.get("startedAt"))),
        "- Timeout: `{}` seconds".format(record.get("timeoutSeconds") or "unknown"),
        "- CWD: `{}`".format(record.get("cwd") or "n/a"),
    ]
    if pane_id:
        lines.append("- {}: `{}`".format(pane_label, pane_id))
    lines.extend(
        [
            "",
            "Use `/dispatch-task watch {}` or `/dispatch-task show {}` to inspect this run.".format(
                record.get("runId") or "run_id", record.get("runId") or "run_id"
            ),
        ]
    )
    return "\n".join(lines)


def ensure_tmux_ready() -> str:
    tmux = shutil.which("tmux")
    if not tmux:
        raise DispatchError("pane monitor requires tmux")
    if not os.environ.get("TMUX"):
        raise DispatchError("current session is not running inside tmux")
    return tmux


def current_tmux_pane(tmux: str) -> str:
    pane = os.environ.get("TMUX_PANE", "").strip()
    if pane:
        return pane
    result = subprocess.run(
        [tmux, "display-message", "-p", "#{pane_id}"],
        capture_output=True,
        text=True,
        check=False,
    )
    pane = result.stdout.strip()
    if result.returncode != 0 or not pane:
        raise DispatchError("current session is not running inside tmux")
    return pane


def launch_tmux_bridge(tmux: str, cwd: str, run_id: str, spec_path: str) -> str:
    current_pane = current_tmux_pane(tmux)
    script_path = str(Path(__file__).resolve())
    bridge_raw = "__bridge-run {}".format(spec_path)
    bridge_command = "cd {cwd} && python3 {script} --cwd {cwd} --raw-arguments {bridge}".format(
        cwd=shlex.quote(cwd),
        script=shlex.quote(script_path),
        bridge=shlex.quote(bridge_raw),
    )
    split = subprocess.run(
        [
            tmux,
            "split-window",
            "-d",
            "-h",
            "-p",
            "40",
            "-P",
            "-F",
            "#{pane_id}",
            bridge_command,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    pane_id = split.stdout.strip()
    if split.returncode != 0 or not pane_id:
        raise DispatchError(
            "Failed to create tmux monitor pane: {}".format(
                split.stderr.strip() or "unknown tmux error"
            )
        )

    subprocess.run(
        [tmux, "select-pane", "-t", pane_id, "-T", "dispatch:{}".format(short_run_id(run_id))],
        capture_output=True,
        text=True,
        check=False,
    )
    subprocess.run(
        [tmux, "select-pane", "-t", current_pane],
        capture_output=True,
        text=True,
        check=False,
    )
    return pane_id


def validate_tmux_monitor() -> None:
    tmux = ensure_tmux_ready()
    current_tmux_pane(tmux)


def build_bridge_env(spec: dict[str, Any]) -> dict[str, str]:
    env = dict(os.environ)
    for key in spec.get("envRemove") or []:
        if isinstance(key, str):
            env.pop(key, None)
    additions = spec.get("env") or {}
    if isinstance(additions, dict):
        for key, value in additions.items():
            if isinstance(key, str) and isinstance(value, str):
                env[key] = value
    path_prefix = spec.get("pathPrefix")
    if isinstance(path_prefix, str) and path_prefix.strip():
        current = env.get("PATH", "")
        env["PATH"] = path_prefix if not current else "{}:{}".format(path_prefix, current)
    return env


def format_callback_for_claude(spec: dict[str, Any], result: str, status: str) -> str:
    callback_block = extract_callback_block_text(result or "")
    if callback_block:
        body = callback_block
    else:
        callback_status = "completed" if status == "succeeded" else "blocked"
        summary = (
            "Bridge run finished without an explicit callback block."
            if status == "succeeded"
            else "Bridge run failed before producing a callback block."
        )
        deliverable = (result or "").strip() or "No deliverable was produced."
        body = "\n".join(
            [
                "<<MAIN_AGENT_CALLBACK>>",
                "status: {}".format(callback_status),
                "message: 我已经实现完了",
                "summary: {}".format(summary),
                "deliverable:",
                deliverable,
                "<</MAIN_AGENT_CALLBACK>>",
            ]
        )

    return "\n".join(
        [
            "子任务已完成，回调如下：",
            "",
            body,
        ]
    )


def send_text_to_tmux_pane(tmux: str, pane_id: str, text: str, submit: bool = True) -> None:
    loaded = subprocess.run(
        [tmux, "load-buffer", "-"],
        input=text,
        text=True,
        capture_output=True,
        check=False,
    )
    if loaded.returncode != 0:
        raise DispatchError(
            "Failed to load tmux buffer: {}".format(loaded.stderr.strip() or "unknown tmux error")
        )

    pasted = subprocess.run(
        [tmux, "paste-buffer", "-t", pane_id],
        capture_output=True,
        text=True,
        check=False,
    )
    if pasted.returncode != 0:
        raise DispatchError(
            "Failed to paste into tmux pane {}: {}".format(
                pane_id, pasted.stderr.strip() or "unknown tmux error"
            )
        )

    if submit:
        submitted = subprocess.run(
            [tmux, "send-keys", "-t", pane_id, "Enter"],
            capture_output=True,
            text=True,
            check=False,
        )
        if submitted.returncode != 0:
            raise DispatchError(
                "Failed to submit callback into tmux pane {}: {}".format(
                    pane_id, submitted.stderr.strip() or "unknown tmux error"
                )
            )


def run_bridge_pane(spec_path: str) -> int:
    tmux = ensure_tmux_ready()
    pane_id = current_tmux_pane(tmux)
    spec = load_bridge_spec(spec_path)
    request_bridge_started(spec, pane_id)

    command = spec.get("command")
    if not isinstance(command, list) or not command:
        raise DispatchError("Bridge spec is missing a valid command.")
    argv = [str(item) for item in command]
    env = build_bridge_env(spec)
    cwd = str(spec.get("cwd") or os.getcwd())
    timeout_seconds = int(spec.get("timeoutSeconds") or DEFAULT_TIMEOUT_SECONDS)
    last_message_path = str(spec.get("lastMessagePath") or "")
    started = time.time()
    stdout_chunks: list[str] = []
    stderr_chunks: list[str] = []

    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    if process.stdout is None or process.stderr is None:
        raise DispatchError("Failed to capture bridge process output streams.")

    def pump(stream: Any, sink: list[str], writer: Any) -> None:
        try:
            for chunk in iter(stream.readline, ""):
                sink.append(chunk)
                writer.write(chunk)
                writer.flush()
        finally:
            stream.close()

    stdout_thread = threading.Thread(
        target=pump, args=(process.stdout, stdout_chunks, sys.stdout), daemon=True
    )
    stderr_thread = threading.Thread(
        target=pump, args=(process.stderr, stderr_chunks, sys.stderr), daemon=True
    )
    stdout_thread.start()
    stderr_thread.start()

    timed_out = False
    while True:
        returncode = process.poll()
        if returncode is not None:
            break
        if time.time() - started > timeout_seconds:
            timed_out = True
            process.kill()
            break
        time.sleep(0.2)

    try:
        returncode = process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        returncode = process.wait()

    stdout_thread.join(timeout=2)
    stderr_thread.join(timeout=2)

    stdout_text = "".join(stdout_chunks).strip()
    stderr_text = "".join(stderr_chunks).strip()
    result = ""
    if last_message_path:
        path = Path(last_message_path)
        if path.exists():
            result = path.read_text(encoding="utf-8").strip()
    if not result:
        result = stdout_text or stderr_text
    if timed_out:
        result = "Dispatch bridge timed out after {} seconds.".format(timeout_seconds)
    if not result:
        result = "Command completed without output."

    status = "timed_out" if timed_out else ("succeeded" if returncode == 0 else "failed")
    duration_ms = max(0, int((time.time() - started) * 1000))
    request_bridge_completed(
        spec,
        {
            "status": status,
            "timedOut": timed_out,
            "cancelled": False,
            "exitCode": None if timed_out else returncode,
            "durationMs": duration_ms,
            "result": result,
            "stdout": stdout_text,
            "stderr": stderr_text,
        },
    )

    if str(spec.get("callbackMode") or "auto") == "auto":
        callback_text = format_callback_for_claude(spec, result, status)
        send_text_to_tmux_pane(tmux, str(spec.get("callbackPane") or ""), callback_text, submit=True)

    print("")
    print("(bridge finished; pane kept open)")
    print("")
    print("Bridge finished. Keeping this pane open. Press Ctrl-C to close it.")
    while True:
        time.sleep(3600)


def tail_lines(text: str, limit: int) -> str:
    lines = text.splitlines()
    if not lines:
        return "(no output yet)"
    return "\n".join(lines[-limit:])


def render_watch_screen(record: dict[str, Any]) -> str:
    columns, rows = shutil.get_terminal_size((120, 40))
    del columns
    status = str(record.get("status") or "unknown")
    started_at = record.get("startedAt")
    finished_at = record.get("finishedAt")
    stdout = str(record.get("stdout") or "").strip()
    stderr = str(record.get("stderr") or "").strip()
    result = str(record.get("result") or "").strip()
    callback = parse_callback_block(result)

    top_lines = [
        "Dispatch Monitor  [{}]".format(short_run_id(str(record.get("runId") or ""))),
        "Status: {}    Target: {}    Provider: {}".format(
            status, record.get("target") or "unknown", record.get("providerName") or "unknown"
        ),
        "Started: {}    Elapsed: {}".format(
            format_timestamp(started_at), format_elapsed(started_at)
        ),
        "CWD: {}".format(record.get("cwd") or "n/a"),
    ]
    if finished_at:
        top_lines.append("Finished: {}".format(format_timestamp(finished_at)))

    budget = max(rows - len(top_lines) - 10, 12)
    stdout_budget = max(budget // 2, 6)
    stderr_budget = max(budget // 4, 4)
    bottom_budget = max(budget - stdout_budget - stderr_budget, 4)

    sections = [
        "\n".join(top_lines),
        "",
        "=== stdout ===",
        tail_lines(stdout, stdout_budget),
        "",
        "=== stderr ===",
        tail_lines(stderr, stderr_budget),
        "",
        "=== callback / result ===",
    ]
    if callback:
        sections.append("status: {}".format(callback["status"] or "_empty_"))
        sections.append("summary: {}".format(callback["summary"] or "_empty_"))
        sections.append("--- deliverable ---")
        sections.append(tail_lines(callback["deliverable"] or "", bottom_budget))
    else:
        sections.append(tail_lines(result, bottom_budget))

    if status in FINISHED_STATUSES:
        sections.extend(["", "(run finished; pane kept open)"])
    else:
        sections.extend(["", "(refreshing every 1s; Ctrl-C to close pane manually)"])
    return "\n".join(sections)


def watch_run(discovery: dict[str, Any], run_id: str) -> int:
    while True:
        record = request_run(discovery, run_id)
        sys.stdout.write("\x1b[2J\x1b[H")
        sys.stdout.write(render_watch_screen(record))
        sys.stdout.flush()
        if str(record.get("status") or "") in FINISHED_STATUSES:
            sys.stdout.write(
                "\n\nRun finished. Keeping this pane open. Press Ctrl-C to close it.\n"
            )
            sys.stdout.flush()
            while True:
                time.sleep(3600)
        time.sleep(1)


def main() -> int:
    arguments = parse_args()

    try:
        raw_arguments = load_raw_arguments(arguments.raw_arguments)
        command = parse_command(raw_arguments)

        if isinstance(command, StatusCommand):
            try:
                discovery = load_discovery()
                print(render_status_from_api(discovery))
            except DispatchError:
                print(render_status_fallback())
            return 0

        if isinstance(command, BridgeRunInternalCommand):
            return run_bridge_pane(command.spec_path)

        discovery = load_discovery()

        if isinstance(command, LastCommand):
            print(render_last(discovery))
            return 0

        if isinstance(command, LogsCommand):
            print(render_logs(discovery, command.limit))
            return 0

        if isinstance(command, ListCommand):
            print(render_list(discovery, command.limit))
            return 0

        if isinstance(command, ShowCommand):
            print(render_record("Dispatch Run", request_run(discovery, command.run_id)))
            return 0

        if isinstance(command, WatchCommand):
            return watch_run(discovery, command.run_id)

        if isinstance(command, CancelCommand):
            payload = request_cancel(discovery, command.run_id)
            run = payload.get("run")
            if not isinstance(run, dict):
                raise DispatchError("Dispatch service returned an invalid cancel payload.")
            print(render_record("Dispatch Cancel", run))
            return 0

        if isinstance(command, ProvidersCommand):
            path = "/v1/dispatch/providers"
            if command.app:
                path += "?{}".format(urllib.parse.urlencode({"app": command.app}))
            payload = request_json(discovery, "GET", path)
            print(render_providers(payload, command.app))
            return 0

        if command.monitor == "pane":
            tmux = ensure_tmux_ready()
            claude_pane = current_tmux_pane(tmux)
            if command.target.split(":", 1)[0] != "codex":
                raise DispatchError("tmux bridge currently requires a codex target")
            payload = request_bridge_prepare(
                discovery,
                {
                    "target": command.target,
                    "task": command.task,
                    "timeoutSeconds": command.timeout_seconds,
                    "cwd": arguments.cwd,
                    "callbackPane": claude_pane,
                    "callbackMode": "auto",
                    "hostApp": "claude",
                },
            )
            spec_path = str(payload.get("specPath") or "").strip()
            if not spec_path:
                raise DispatchError("Dispatch service did not return a bridge spec path.")
            pane_id = launch_tmux_bridge(
                tmux,
                arguments.cwd,
                str(payload.get("runId") or ""),
                spec_path,
            )
            print(render_started(payload, pane_id=pane_id, pane_label="Bridge pane"))
            return 0

        payload = request_json(
            discovery,
            "POST",
            "/v1/dispatch/run",
            payload={
                "target": command.target,
                "task": command.task,
                "timeoutSeconds": command.timeout_seconds,
                "waitForCompletion": command.wait_for_completion,
                "cwd": arguments.cwd,
            },
            timeout_seconds=max(command.timeout_seconds + 15, 30),
        )
        if payload.get("completed"):
            print(render_record("Dispatch Result", payload))
            return 0

        print(render_started(payload))
        return 0
    except DispatchError as exc:
        print(render_failure(str(exc)))
        return 0
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
