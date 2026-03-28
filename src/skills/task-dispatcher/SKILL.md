---
name: dispatch-task
description: Dispatch a subtask to a Claude or Codex provider configured in cc-switch, inspect dispatch status/history inside Claude Code, wait for the result to come back to the current Claude Code session, and require the child agent to callback to the main agent when finished. Use for `/dispatch-task providers [app]`, `/dispatch-task status`, `/dispatch-task last`, `/dispatch-task logs [count]`, or `/dispatch-task <claude|codex:provider_id> [timeout=<seconds>] -- <task text>`.
disable-model-invocation: true
user-invocable: true
context: fork
allowed-tools:
  - Bash(python3 *)
---

Use the bundled helper to talk to cc-switch's loopback dispatch service and local dispatch state files. Do not attempt to solve the requested subtask yourself.

Run this exact command once from the current project directory:

```bash
python3 "${CLAUDE_SKILL_DIR}/scripts/dispatch.py" --cwd "$PWD" <<'EOF'
$ARGUMENTS
EOF
```

Supported commands:

- `/dispatch-task providers [app]`
- `/dispatch-task status`
- `/dispatch-task last`
- `/dispatch-task logs [count]`
- `/dispatch-task <app:provider_id> [timeout=<seconds>] -- <task text>`

Treat the helper output as the source of truth, then return that output as your entire response without adding commentary, reformulation, or extra steps.
