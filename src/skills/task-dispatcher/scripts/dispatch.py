#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import sys
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
class RunCommand:
    target: str
    timeout_seconds: int
    task: str


Command = Union[ProvidersCommand, StatusCommand, LastCommand, LogsCommand, RunCommand]


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
        "- /dispatch-task <app:provider> [timeout=<seconds>] -- <task text>",
    ]


def usage_error(message: str) -> str:
    return "{}\n\nUsage:\n{}".format(message, "\n".join(usage_lines()))


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
        limit = DEFAULT_LOG_LIMIT
        if len(tokens) == 2:
            try:
                limit = int(tokens[1], 10)
            except ValueError as exc:
                raise DispatchError("`logs` count must be an integer.") from exc
            if limit < 1 or limit > MAX_LOG_LIMIT:
                raise DispatchError(
                    "`logs` count must be between 1 and {}.".format(MAX_LOG_LIMIT)
                )
        return LogsCommand(limit=limit)

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
    for token in head_tokens[1:]:
        if not token.startswith("timeout="):
            raise DispatchError(
                "Unexpected argument '{}'. Only timeout=<seconds> is supported.".format(
                    token
                )
            )
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

    return RunCommand(
        target="{}:{}".format(app, provider_id),
        timeout_seconds=timeout_seconds,
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
    base_url = str(discovery["baseUrl"]).rstrip("/")
    url = base_url + path
    data = None
    headers = {
        "Authorization": "Bearer {}".format(discovery["token"]),
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


def strip_callback_block(text: str) -> str:
    normalized = text.replace("\r\n", "\n")
    return CALLBACK_BLOCK_RE.sub("", normalized).strip()


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


def render_status() -> str:
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
            lines.append("- Target: `{}`".format(current_run.get("target") or "unknown"))
            lines.append("- Provider: `{}`".format(current_run.get("providerName") or "unknown"))
            lines.append("- Started: `{}`".format(format_timestamp(current_run.get("startedAt"))))
            lines.append("- Timeout: `{}` seconds".format(current_run.get("timeoutSeconds") or "unknown"))
            lines.append("- CWD: `{}`".format(current_run.get("cwd") or "n/a"))

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
    return [
        "- Last target: `{}`".format(entry.get("target") or "unknown"),
        "- Last outcome: `{}`".format(entry.get("status") or "unknown"),
        "- Last callback: `{}`".format(callback_status),
        "- Last finished: `{}`".format(format_timestamp(entry.get("timestamp"))),
        "- Last duration: `{}`".format(format_duration(entry.get("durationMs"))),
    ]


def render_last() -> str:
    entries = load_history_entries(1)
    if not entries:
        return "## Dispatch Last Run\n\n- No dispatch history is available yet."
    return render_record("Dispatch Last Run", entries[0])


def render_logs(limit: int) -> str:
    entries = load_history_entries(limit)
    lines = ["## Dispatch Logs", "", "- Showing the newest `{}` entr{}.".format(limit, "y" if limit == 1 else "ies")]

    if not entries:
        lines.append("- No dispatch history is available yet.")
        return "\n".join(lines)

    for entry in entries:
        callback = parse_callback_block(str(entry.get("result") or ""))
        callback_status = callback["status"] if callback else "missing"
        lines.append(
            "- `{}` | `{}` | `{}` | `{}` | callback=`{}` | exit=`{}`".format(
                format_timestamp(entry.get("timestamp")),
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


def render_record(title: str, record: dict[str, Any]) -> str:
    timed_out = bool(record.get("timedOut"))
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

    lines = [
        "## {}".format(title),
        "",
        "- Outcome: `{}`".format(outcome),
        "- Target: `{}`".format(target),
        "- Provider: `{}`".format(provider_name),
        "- Duration: `{}`".format(duration),
        "- Exit code: `{}`".format(exit_code if exit_code is not None else "n/a"),
        "- Callback: `{}`".format(callback["status"] if callback else "missing"),
    ]

    timestamp = record.get("timestamp")
    if timestamp is not None:
        lines.append("- Finished: `{}`".format(format_timestamp(timestamp)))
    if cwd:
        lines.append("- CWD: `{}`".format(cwd))

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
    elif stdout and stdout != result and not bool(record.get("ok")):
        lines.extend(["", "### stdout", fenced_block(stdout)])

    return "\n".join(lines)


def fenced_block(text: str) -> str:
    return "```text\n{}\n```".format(text)


def render_failure(message: str) -> str:
    return "## Dispatch Error\n\n- Error: {}".format(message.strip())


def main() -> int:
    arguments = parse_args()

    try:
        raw_arguments = load_raw_arguments(arguments.raw_arguments)
        command = parse_command(raw_arguments)

        if isinstance(command, StatusCommand):
            print(render_status())
            return 0

        if isinstance(command, LastCommand):
            print(render_last())
            return 0

        if isinstance(command, LogsCommand):
            print(render_logs(command.limit))
            return 0

        discovery = load_discovery()

        if isinstance(command, ProvidersCommand):
            path = "/v1/dispatch/providers"
            if command.app:
                path += "?{}".format(urllib.parse.urlencode({"app": command.app}))
            payload = request_json(discovery, "GET", path)
            print(render_providers(payload, command.app))
            return 0

        payload = request_json(
            discovery,
            "POST",
            "/v1/dispatch/run",
            payload={
                "target": command.target,
                "task": command.task,
                "timeoutSeconds": command.timeout_seconds,
                "cwd": arguments.cwd,
            },
            timeout_seconds=max(command.timeout_seconds + 15, 30),
        )
        print(render_record("Dispatch Result", payload))
        return 0
    except DispatchError as exc:
        print(render_failure(str(exc)))
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
