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
from pathlib import Path
from typing import Any

DISCOVERY_PATH = Path.home() / ".cc-switch" / "dispatch-api.json"
DEFAULT_TIMEOUT_SECONDS = 120
MAX_TIMEOUT_SECONDS = 900
SUPPORTED_APPS = ("claude", "codex")


class DispatchError(RuntimeError):
    pass


@dataclass
class ProvidersCommand:
    app: str | None


@dataclass
class RunCommand:
    target: str
    timeout_seconds: int
    task: str


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


def load_raw_arguments(raw_arguments: str | None) -> str:
    raw = raw_arguments if raw_arguments is not None else sys.stdin.read()
    raw = raw.strip()
    if not raw:
        raise DispatchError(
            "Missing dispatch arguments.\n\n"
            "Usage:\n"
            "- /dispatch-task providers [app]\n"
            "- /dispatch-task <app:provider_id> [timeout=<seconds>] -- <task text>"
        )
    return raw


def parse_command(raw: str) -> ProvidersCommand | RunCommand:
    tokens = shlex.split(raw)
    if not tokens:
        raise DispatchError("Missing dispatch arguments.")

    if tokens[0] == "providers":
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

    match = re.match(r"(?s)^(?P<head>.+?)\s+--\s*(?P<task>.+)$", raw)
    if not match:
        raise DispatchError(
            "Missing `-- <task text>` separator.\n\n"
            "Usage:\n"
            "- /dispatch-task providers [app]\n"
            "- /dispatch-task <app:provider_id> [timeout=<seconds>] -- <task text>"
        )

    head = match.group("head").strip()
    task = match.group("task").strip()
    if not task:
        raise DispatchError("Task text cannot be empty.")

    head_tokens = shlex.split(head)
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
    app, separator, provider_id = target.partition(":")
    app = app.strip().lower()
    provider_id = provider_id.strip()

    if separator != ":" or not provider_id:
        raise DispatchError(
            "Target must use the form `claude:<provider_id>` or `codex:<provider_id>`."
        )
    if app not in SUPPORTED_APPS:
        raise DispatchError(
            "Unsupported target app '{}'. Expected one of: {}.".format(
                app, ", ".join(SUPPORTED_APPS)
            )
        )

    return app, provider_id


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
    payload: dict[str, Any] | None = None,
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


def format_duration(duration_ms: Any) -> str:
    if not isinstance(duration_ms, (int, float)):
        return "unknown"

    seconds = float(duration_ms) / 1000.0
    if seconds >= 10:
        return "{:.1f}s".format(seconds)
    return "{:.2f}s".format(seconds)


def render_providers(payload: dict[str, Any], app_filter: str | None) -> str:
    providers = payload.get("providers")
    if not isinstance(providers, list):
        raise DispatchError("Dispatch service returned an invalid providers payload.")

    title = "Available dispatch targets"
    if app_filter:
        title += " ({})".format(app_filter)

    lines = [title, ""]

    if not providers:
        lines.append("- No dispatchable providers found in cc-switch.")
        lines.append("- Supported apps in v1: claude, codex.")
    else:
        for item in providers:
            if not isinstance(item, dict):
                continue
            target = str(item.get("target") or "unknown")
            name = str(item.get("providerName") or item.get("providerId") or "unknown")
            suffix = " (current)" if item.get("current") else ""
            lines.append("- {} - {}{}".format(target, name, suffix))

    lines.extend(
        [
            "",
            "Usage:",
            "- /dispatch-task providers [app]",
            "- /dispatch-task <app:provider_id> [timeout=<seconds>] -- <task text>",
        ]
    )
    return "\n".join(lines)


def render_run(payload: dict[str, Any]) -> str:
    ok = bool(payload.get("ok"))
    timed_out = bool(payload.get("timedOut"))
    status = str(payload.get("status") or "unknown")
    target = str(payload.get("target") or "unknown")
    provider_name = str(payload.get("providerName") or "unknown")
    exit_code = payload.get("exitCode")
    duration = format_duration(payload.get("durationMs"))
    cwd = str(payload.get("cwd") or "")
    result = str(payload.get("result") or "").strip()
    stdout = str(payload.get("stdout") or "").strip()
    stderr = str(payload.get("stderr") or "").strip()

    if ok:
        title = "Dispatch succeeded"
    elif timed_out:
        title = "Dispatch timed out"
    else:
        title = "Dispatch failed"

    lines = [
        title,
        "",
        "- Target: {}".format(target),
        "- Provider: {}".format(provider_name),
        "- Status: {}".format(status),
        "- Duration: {}".format(duration),
        "- Exit code: {}".format(exit_code if exit_code is not None else "n/a"),
        "- CWD: {}".format(cwd or "n/a"),
    ]

    if result:
        lines.extend(["", "Result", result])

    if stderr and stderr != result:
        lines.extend(["", "stderr", stderr])
    elif stdout and stdout != result and not ok:
        lines.extend(["", "stdout", stdout])

    return "\n".join(lines)


def render_failure(message: str) -> str:
    return "Dispatch request could not be completed.\n\n- Error: {}".format(message.strip())


def main() -> int:
    arguments = parse_args()

    try:
        raw_arguments = load_raw_arguments(arguments.raw_arguments)
        command = parse_command(raw_arguments)
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
        print(render_run(payload))
        return 0
    except DispatchError as exc:
        print(render_failure(str(exc)))
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
