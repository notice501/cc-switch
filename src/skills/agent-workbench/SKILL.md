---
name: agent
description: Plan or run a child agent with suggested routing, tmux pane runtime support, and Claude/Codex execution policies. Use for `agent plan --task "..."`, `agent run --task "..."`, `agent list`, `agent show <run_id>`, `agent watch <run_id>`, `agent attach <run_id>`, `agent cancel <run_id>`, `agent providers [app]`, and `agent status`.
---

Use the bundled helper to talk to the local agent runtime service. Do not solve the subtask yourself unless the caller explicitly asked you to stop using the runtime.

```bash
python3 "${CLAUDE_SKILL_DIR}/scripts/agent.py" --cwd "$PWD" <<'EOF'
$ARGUMENTS
EOF
```

Supported commands:

- `agent plan --task "..." [--policy <name>] [--target <app:provider>] [--mode pane|background|inline]`
- `agent run --task "..." [--policy <name>] [--target <app:provider>] [--mode pane|background|inline] [--timeout <seconds>] [--wait]`
- `agent list [count]`
- `agent show <run_id>`
- `agent watch <run_id>`
- `agent attach <run_id>`
- `agent cancel <run_id>`
- `agent providers [app]`
- `agent status`

Notes:

- `pane` mode requires tmux and currently expects a Codex child pane.
- `--wait` blocks the current Claude turn and cannot be combined with `--mode pane`.
- If the target is omitted, the runtime will suggest a route and pick a provider automatically.
- `providers` is a low-level escape hatch; the main flow is `plan` then `run`.
