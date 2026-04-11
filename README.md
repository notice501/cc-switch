<div align="center">

# CCswitch Pro

### Turn multiple coding plans and model tiers into one smoother workflow

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/notice501/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

English | [中文](README_ZH.md) | [日本語](README_JA.md) | [Changelog](CHANGELOG.md) | [Upstream Project](https://github.com/farion1231/cc-switch)

</div>

## Why This Exists

Many people do not have just one model subscription anymore.

You might have Aliyun coding plans, Zhipu coding plans, official Claude access, Codex access, or several relay providers. Each one has different tradeoffs, but in a normal CLI workflow you often end up using only one at a time because switching is annoying and the others sit idle.

The second pain point is model division of labor. Opus is powerful and great for architecture, planning, and judgment, but it is expensive. It is often not the model you want doing every implementation detail from start to finish. A more natural workflow is: **use the expensive model for decisions, and cheaper models for execution**.

CCswitch Pro is built around that idea. The goal is not just “switch providers,” but to help you organize multiple providers, plans, and model tiers into a workflow that is actually comfortable to use.

## Two Common Workflows

### Workflow A: use multiple subscriptions instead of picking one forever

- You have Aliyun coding plans, Zhipu coding plans, or multiple Claude / Codex providers
- Different providers vary in cost, stability, and model availability
- You do not want to hand-edit config files just to move between them
- You want to use whichever provider is best for the current job, without friction

### Workflow B: let stronger models plan and cheaper models execute

- Let Opus handle architecture, task breakdown, and hard judgment calls
- Let Codex or cheaper models handle implementation details and repetitive code work
- Keep the expensive model focused on high-value thinking
- Treat models more like a team with roles, not one monolithic assistant

## How CCswitch Pro Helps

- Manage providers for Claude Code, Codex, Gemini CLI, OpenCode, and OpenClaw from one UI
- Switch the current provider without hand-editing JSON, TOML, or `.env`
- Dispatch subtasks to specific **Claude** or **Codex** providers when a different model is a better fit
- Resolve dispatch targets by `alias`, provider name, raw provider id, or `current`
- The collaboration story here is centered on the **Claude + Codex dispatch path** today, not a built-in full auto-orchestrator

## Why Not Just Basic Switching?

Basic provider switching answers “which one am I using right now?” but not “how do I make several subscriptions useful together?”

CCswitch Pro is more opinionated about real multi-provider workflows: keep several plans around, switch quickly, and send some tasks to better-fit Claude or Codex providers when needed.

That is the main reason this fork exists: not to repaint the upstream app, but to push harder toward practical multi-model collaboration.

## What This Fork Added

Beyond the upstream base, this fork now adds several workflow-oriented changes that materially change how it is used day to day:

- A fully isolated **Pro** flavor with its own app identity, config root, local storage namespace, and deep-link scheme
- **Claude alias launchers** such as `claude-dou`, `claude-kimi`, and similar wrappers, so different providers can be launched directly as separate working entries
- A dedicated local **dispatch service** for routing child tasks to Claude or Codex providers
- **Background async dispatch** with persisted run records instead of one blocking child call
- A **tmux bridge mode** that can open a real Codex pane, run the sub-agent there, and automatically send the callback back to the current Claude pane
- Better **dispatch observability** through status line integration, recent-run inspection, run details, and tail-style watching

In short: upstream remains the base for provider management, while this fork pushes harder into “one expensive planner + one cheaper executor” style workflows.

## Core Capabilities

[Full Changelog](CHANGELOG.md) | [Release Notes](docs/release-notes/v3.12.3-en.md)

### Multi-subscription management

- Manage providers across 5 CLI tools in one place
- Import from 50+ presets to onboard different coding plans and relay providers quickly
- Use sorting, import/export, and tray switching to reduce switching friction

### Multi-model collaboration and task dispatch

- Keep one provider as your main entry point, then dispatch subtasks to a better-fit Claude or Codex provider
- Resolve dispatch targets through `alias`, provider name, provider id, or `current`
- Dispatch runs in the background by default, with persisted run state, history, and callback inspection
- Optional `tmux` bridge mode lets a real Codex pane execute the child task while the current Claude pane stays usable
- A good fit for workflows like “Opus for architecture, Codex for implementation,” without pretending to be a fully automatic orchestrator

### Cost and usage control

- Track requests, tokens, and spend per provider
- Keep expensive models for high-value reasoning and move repetitive execution to cheaper models
- Use custom pricing to compare the real cost of different plans

### Sync and isolation

- Cloud sync, custom config directories, backups, and atomic writes help keep setups durable
- This fork uses its own config directory, deep-link scheme, and local storage namespace
- Claude alias launchers also use isolated per-alias homes, so provider-specific runtime state does not get mixed together
- Easier to run side-by-side with upstream builds without mixing state by accident

### MCP, Prompts, and Skills support

- Manage MCP, Prompts, and Skills centrally instead of redoing the same setup per CLI
- Sync prompt files across apps with backfill protection
- Install Skills from GitHub repos or ZIP archives to capture repeatable workflows

## Screenshots

|                  Main Interface                   |                  Add Provider                  |
| :-----------------------------------------------: | :--------------------------------------------: |
| ![Main Interface](assets/screenshots/main-en.png) | ![Add Provider](assets/screenshots/add-en.png) |

## Quick Start

1. Add the providers you actually use, such as Aliyun, Zhipu, official Claude / Codex, or relay-based coding plans
2. Pick one provider as your current working entry point
3. When a task needs stronger reasoning, switch to a stronger provider; when it needs lower-cost execution, dispatch subtasks to a cheaper Claude or Codex provider
4. Use the usage panel to refine your own model split over time

> **Note**: On first launch you can import existing CLI configs as your default provider set. The current dispatch path is primarily for Claude and Codex providers.

## A More Accurate Current Workflow

The most complete workflow in this fork today is:

1. Use **Claude Code** as the main pane where you plan, review, and make decisions
2. Keep **Codex** available as a child executor for implementation-heavy subtasks
3. Use `/dispatch-task` to send a subtask to a specific Claude or Codex provider
4. For long implementation tasks, use `monitor=pane` inside `tmux` so a real Codex pane runs beside your current Claude pane
5. Let the child finish, then inspect or reuse the callback from `last`, `show`, or the automatic pane callback

That is the most important practical change in this fork: not only switching providers, but turning multiple subscriptions into a more deliberate collaboration loop.

## How To Use The Dispatch Skill

The dispatch workflow is currently exposed through the built-in **`/dispatch-task` skill inside Claude Code**. In practice:

- CCswitch Pro manages providers and runs the local dispatch service
- Claude Code invokes the built-in `/dispatch-task` skill
- Dispatch targets currently support **Claude providers** and **Codex providers**

### Recommended flow

1. Configure your Claude and Codex providers in CCswitch Pro
2. Keep the CCswitch Pro desktop app running
3. In Claude Code, run `/dispatch-task providers` first to see available targets
4. Pick a target and dispatch the subtask
5. For longer implementation tasks, optionally add `monitor=pane` inside `tmux`
6. Use `status`, `list`, `show`, `last`, and `logs` to inspect the run or fetch the final result

### Command format

```text
/dispatch-task providers [app]
/dispatch-task status
/dispatch-task last
/dispatch-task logs [count]
/dispatch-task list [count]
/dispatch-task show <run_id>
/dispatch-task watch <run_id>
/dispatch-task cancel <run_id>
/dispatch-task <app:provider> [timeout=<seconds>] [wait=true] [monitor=pane|none] -- <task text>
```

Notes:

- `app` currently supports `claude` and `codex`
- `provider` can be an `alias`, provider name, provider id, or `current`
- Default timeout is `120` seconds and the max is `900`
- Dispatch runs in the background by default; add `wait=true` if you want the current Claude Code session to block until the child finishes
- `monitor=pane` requires `tmux`, must be run from inside a `tmux` pane, and currently targets a real **Codex** pane workflow
- `wait=true` cannot be combined with `monitor=pane`
- The actual task must appear after `--`

### Common commands

#### 1. List dispatch targets

```text
/dispatch-task providers
/dispatch-task providers claude
/dispatch-task providers codex
```

This prints the targets you can actually dispatch to, such as:

- `claude:current`
- `claude:opus`
- `codex:aliyun-codex`
- `codex:zhipu-codex`

If you are unsure how a target should be spelled, start here.

#### 2. Check current status

```text
/dispatch-task status
```

Useful for checking whether a run is still in progress, which target it is using, and how long it has been running.

#### 3. Inspect the latest result

```text
/dispatch-task last
```

Useful for reviewing the latest callback, summary, and deliverable from the child run.

#### 4. Review recent history

```text
/dispatch-task logs
/dispatch-task logs 5
```

Useful for quickly seeing whether recent dispatches succeeded, who they were sent to, and whether anything timed out.

#### 5. Dispatch a real subtask

```text
/dispatch-task claude:current -- First do the architecture analysis and provide a task breakdown.
/dispatch-task claude:opus -- Evaluate the boundary, risks, and module split for this feature.
/dispatch-task codex:current -- Implement the first part directly based on the agreed plan.
/dispatch-task codex:aliyun-codex timeout=600 -- Implement the API layer and tests based on the breakdown above.
/dispatch-task codex:aliyun-codex timeout=600 wait=true -- Block until the API layer and tests are done.
/dispatch-task codex:current monitor=pane -- Implement the child task in a real tmux Codex pane and callback automatically when done.
```

#### 6. List recent runs

```text
/dispatch-task list
/dispatch-task list 10
```

Useful for quickly seeing the most recent running, succeeded, failed, timed-out, or cancelled child runs.

#### 7. Inspect or watch one run

```text
/dispatch-task show <run_id>
/dispatch-task watch <run_id>
```

Use `show` when you want the stored summary, stdout/stderr tails, callback, and deliverable. Use `watch` when you want a live terminal-style refresh for one run.

#### 8. Cancel a running run

```text
/dispatch-task cancel <run_id>
```

Useful when a child run is no longer needed, has drifted, or is obviously taking the wrong path.

### Example workflow

#### Opus for architecture, Codex for implementation

1. Inspect available targets

```text
/dispatch-task providers
```

2. Ask a stronger Claude provider to produce the architecture breakdown

```text
/dispatch-task claude:opus -- Read the current project structure and produce module boundaries, risks, and the recommended implementation order.
```

3. Hand execution to a Codex provider

```text
/dispatch-task codex:aliyun-codex -- Based on the breakdown above, implement phase one and add the necessary tests.
```

4. Inspect status and history

```text
/dispatch-task status
/dispatch-task list
/dispatch-task last
/dispatch-task logs 5
```

#### Rotating between multiple coding plans

If you keep both Aliyun and Zhipu coding plans available in CCswitch Pro, you can route different subtasks to different Codex providers as needed:

```text
/dispatch-task codex:aliyun-codex -- Handle the larger batch implementation task first.
/dispatch-task codex:zhipu-codex -- Then handle the next subtask that fits this provider better.
```

#### Real tmux bridge workflow

If you are working inside `tmux`, you can keep Claude in the current pane and open a real Codex child pane to execute the subtask:

```text
/dispatch-task codex:current monitor=pane -- Implement the child task in the adjacent Codex pane and send the callback back when finished.
```

This is different from a passive monitor. The right pane is an actual child Codex execution pane, not just a log viewer.

### Practical tips

- If you are unsure about the target syntax, run `/dispatch-task providers` first
- To use the currently active provider, write `claude:current` or `codex:current`
- For longer jobs, explicitly add `timeout=600`
- If you want the split-pane workflow, start from inside `tmux` and use `monitor=pane`
- If you see a dispatch-service-not-found error, make sure the **CCswitch Pro desktop app is running**

## Installation

### System Requirements

- **Windows**: Windows 10 and above
- **macOS**: macOS 12 (Monterey) and above
- **Linux**: Ubuntu 22.04+ / Debian 11+ / Fedora 34+ and other mainstream distributions

### Install

Download the appropriate build for your platform from the [Releases](../../releases) page.

- **Windows**: MSI installer or portable ZIP
- **macOS**: app bundle ZIP from the release page
- **Linux**: `.deb`, `.rpm`, `.AppImage`, or `.flatpak`

> **Note**: This fork currently documents manual installation only. Package manager distribution should come later, once the fork's own release pipeline is stable.

## FAQ

<details>
<summary><strong>What kinds of workflows is CCswitch Pro best for?</strong></summary>

It is best for two cases: you have several coding plans or providers and want to use them all efficiently, or you want stronger models to handle architecture and judgment while cheaper models handle implementation and execution.

</details>

<details>
<summary><strong>Can I use Opus for architecture and Codex for implementation?</strong></summary>

Yes, that is exactly the kind of workflow this project is meant to support. You can manage multiple providers and dispatch subtasks to specific Claude or Codex providers. It does not automatically orchestrate that split for you, but it makes the workflow much easier to run.

</details>

<details>
<summary><strong>Is this a full automatic multi-model orchestrator?</strong></summary>

No. Think of it more as a control surface for organizing providers, switching quickly, and building Claude / Codex dispatch-based workflows, rather than a system that makes all orchestration decisions automatically.

</details>

<details>
<summary><strong>Do I need to restart the terminal after switching providers?</strong></summary>

For most tools, yes. Restart the terminal or the CLI tool for changes to take effect. The exception is **Claude Code**, which currently supports hot-switching provider data without a restart.

</details>

<details>
<summary><strong>Where is my data stored?</strong></summary>

- **Database**: `~/.ccswitch-pro/cc-switch.db`
- **Local settings**: `~/.ccswitch-pro/settings.json`
- **Backups**: `~/.ccswitch-pro/backups/`
- **Claude alias homes**: `~/.ccswitch-pro/alias-homes/<alias>/`
- **Dispatch status**: `~/.ccswitch-pro/dispatch-status.json`
- **Dispatch history**: `~/.ccswitch-pro/dispatch-history.jsonl`
- **Skills**: `~/.ccswitch-pro/skills/`
- **Skill backups**: `~/.ccswitch-pro/skill-backups/`

This fork also uses its own deep-link scheme, `ccswitchpro://`, plus its own local storage namespace so it is easier to run beside upstream builds.

</details>

<details>
<summary><strong>Why use this instead of just using upstream switching?</strong></summary>

Upstream already provides a strong base for provider management. This fork leans harder into multi-provider collaboration workflows, dispatch ergonomics, and side-by-side isolation. If your main goal is to make several subscriptions and model tiers work together more smoothly, this version is aimed at that use case.

</details>

## Documentation

For more detailed usage guidance, check the **[User Manual](docs/user-manual/en/README.md)** covering providers, MCP, Prompts, Skills, proxy features, and failover.

## Fork Lineage

**CCswitch Pro** is an independently maintained fork of [CC Switch](https://github.com/farion1231/cc-switch).

It keeps the upstream foundation, while adding fork-specific isolation and workflow-oriented refinements such as a separate app identity, separate config directory, separate WebDAV root, separate local storage prefix, separate deep-link scheme, and more ergonomic Claude / Codex dispatch targeting.

<details>
<summary><strong>Architecture Overview</strong></summary>

### Design Principles

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (React + TS)                    │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   (UI)      │──│ (Bus. Logic) │──│   (Cache/Sync)   │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────────┐
│                  Backend (Tauri + Rust)                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │  Models/Config   │    │
│  │ (API Layer) │──│ (Bus. Layer) │──│     (Data)       │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Core Design Patterns**

- **SSOT** (Single Source of Truth): All data stored in `~/.ccswitch-pro/cc-switch.db` (SQLite)
- **Dual-layer Storage**: SQLite for syncable data, JSON for device-level settings
- **Dual-way Sync**: Write to live files on switch, backfill from live when editing active provider
- **Atomic Writes**: Temp file + rename pattern prevents config corruption
- **Concurrency Safe**: Mutex-protected database connection avoids race conditions
- **Layered Architecture**: Clear separation (Commands → Services → DAO → Database)

**Key Components**

- **ProviderService**: Provider CRUD, switching, backfill, sorting
- **McpService**: MCP server management, import/export, live file sync
- **ProxyService**: Local proxy mode with hot-switching and format conversion
- **SessionManager**: Claude Code conversation history browsing
- **ConfigService**: Config import/export, backup rotation
- **SpeedtestService**: API endpoint latency measurement

</details>

<details>
<summary><strong>Development Guide</strong></summary>

### Environment Requirements

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### Development Commands

```bash
# Install dependencies
pnpm install

# Dev mode (hot reload)
pnpm dev

# Type check
pnpm typecheck

# Format code
pnpm format

# Check code format
pnpm format:check

# Run frontend unit tests
pnpm test:unit

# Run tests in watch mode (recommended for development)
pnpm test:unit:watch

# Build application
pnpm build

# Build debug version
pnpm tauri build --debug
```

### Rust Backend Development

```bash
cd src-tauri

# Format Rust code
cargo fmt

# Run clippy checks
cargo clippy

# Run backend tests
cargo test

# Run specific tests
cargo test test_name

# Run tests with test-hooks feature
cargo test --features test-hooks
```

### Testing Guide

**Frontend Testing**:

- Uses **vitest** as test framework
- Uses **MSW (Mock Service Worker)** to mock Tauri API calls
- Uses **@testing-library/react** for component testing

**Running Tests**:

```bash
# Run all tests
pnpm test:unit

# Watch mode (auto re-run)
pnpm test:unit:watch

# With coverage report
pnpm test:unit --coverage
```

### Tech Stack

**Frontend**: React 18 · TypeScript · Vite · TailwindCSS 3.4 · TanStack Query v5 · react-i18next · react-hook-form · zod · shadcn/ui · @dnd-kit

**Backend**: Tauri 2.8 · Rust · serde · tokio · thiserror · tauri-plugin-updater/process/dialog/store/log

**Testing**: vitest · MSW · @testing-library/react

</details>

<details>
<summary><strong>Project Structure</strong></summary>

```
├── src/                        # Frontend (React + TypeScript)
│   ├── components/
│   │   ├── providers/          # Provider management
│   │   ├── mcp/                # MCP panel
│   │   ├── prompts/            # Prompts management
│   │   ├── skills/             # Skills management
│   │   ├── sessions/           # Session Manager
│   │   ├── proxy/              # Proxy mode panel
│   │   ├── openclaw/           # OpenClaw config panels
│   │   ├── settings/           # Settings (Terminal/Backup/About)
│   │   ├── deeplink/           # Deep Link import
│   │   ├── env/                # Environment variable management
│   │   ├── universal/          # Cross-app configuration
│   │   ├── usage/              # Usage statistics
│   │   └── ui/                 # shadcn/ui component library
│   ├── hooks/                  # Custom hooks (business logic)
│   ├── lib/
│   │   ├── api/                # Tauri API wrapper (type-safe)
│   │   └── query/              # TanStack Query config
│   ├── locales/                # Translations (zh/en/ja)
│   ├── config/                 # Presets (providers/mcp)
│   └── types/                  # TypeScript definitions
├── src-tauri/                  # Backend (Rust)
│   └── src/
│       ├── commands/           # Tauri command layer (by domain)
│       ├── services/           # Business logic layer
│       ├── database/           # SQLite DAO layer
│       ├── proxy/              # Proxy module
│       ├── session_manager/    # Session management
│       ├── deeplink/           # Deep Link handling
│       └── mcp/                # MCP sync module
├── tests/                      # Frontend tests
└── assets/                     # Screenshots & partner resources
```

</details>

## Contributing

Issues and suggestions are welcome!

Before submitting PRs, please ensure:

- Pass type check: `pnpm typecheck`
- Pass format check: `pnpm format:check`
- Pass unit tests: `pnpm test:unit`

For new features, please open an issue for discussion before submitting a PR. PRs for features that are not a good fit for the project may be closed.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=notice501/cc-switch&type=Date)](https://www.star-history.com/#notice501/cc-switch&Date)

## License

MIT © Jason Young
