<div align="center">

# CCswitch Pro

### 把多个 Coding Plan 和不同模型，组织成一个更顺手的工作流

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/notice501/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

[English](README.md) | 中文 | [日本語](README_JA.md) | [更新日志](CHANGELOG.md) | [上游项目](https://github.com/farion1231/cc-switch)

</div>

## 为什么做这个

很多人手上不只一个模型订阅。

你可能同时订阅了阿里云、智谱，或者不同渠道的 Claude / Codex coding plan。它们各有优劣，但日常 CLI 工作流里通常只能顺手用一个，另外几个就容易变成“买了但闲置”。

另一个常见问题是模型分工。Opus 很强，适合做架构判断、方案设计、拆任务；但它贵，不适合从头到尾都拿来写细节。更自然的方式应该是：**贵模型负责判断，便宜模型负责执行**。

CCswitch Pro 想解决的就是这个问题：不是只让你“切换 provider”，而是让你把多个 provider、多个 plan、不同价位和能力的模型，组织成一个真正能协作的工作流。

## 两个典型使用场景

### 场景 A：多个订阅一起用

- 你同时有阿里云 coding plan、智谱 coding plan，或者多个 Claude / Codex provider
- 不同 provider 在稳定性、价格、模型可用性上不一样
- 你不想每次手改配置，也不想固定只用其中一个
- 你希望哪个更适合当前任务，就顺手切过去用

### 场景 B：强模型做架构，便宜模型做实现

- 让 Opus 先做架构设计、任务拆解、方案评估
- 把细节实现、批量改代码、重复性执行交给 Codex 或更便宜的模型
- 主模型保留高价值判断，执行部分按成本优化
- 这样更接近真实团队协作，而不是“一个模型包办一切”

## CCswitch Pro 怎么解决

- 把多个 Claude / Codex / Gemini / OpenCode / OpenClaw provider 放在一个界面里统一管理
- 需要切换当前 provider 时，不用再手改 JSON、TOML 或 `.env`
- 针对 **Claude** 和 **Codex**，可以通过 dispatch 把子任务发给指定 provider
- dispatch 目标支持 `alias`、provider 名称、原始 provider id，或者 `current`
- 当前这套“多模型协作”能力主要围绕 **Claude + Codex 的分发链路**，不是一个内置全自动编排器

## 为什么不是只做普通切换

普通的“切换 provider”解决的是“现在用哪个”，但没有真正解决“多个订阅如何一起发挥价值”。

CCswitch Pro 更偏向一种多实例、多 provider、多模型协作的工作流：你可以保留多个 plan，快速切换当前使用的 provider，并把某些任务继续分发给更合适的 Claude 或 Codex provider。

这也是这个 fork 和上游分化出来的重点：不是换个名字，而是把重点放在更适合真实多模型协作的使用方式上。

## 核心能力

[完整更新日志](CHANGELOG.md) | [发布说明](docs/release-notes/v3.12.3-zh.md)

### 多订阅管理

- 在一个应用里管理 5 个 CLI 工具的 provider
- 内置 50+ 预设，适合快速导入不同 coding plan 或中转 provider
- 支持拖拽排序、导入导出、托盘切换，减少“切来切去”的摩擦

### 多模型协作与任务分发

- 把当前 provider 当作主工作入口，需要时再把子任务 dispatch 给 Claude 或 Codex 的其他 provider
- dispatch 目标支持 `alias`、provider 名、provider id、`current`
- 适合 “Opus 负责架构 / Codex 负责实现” 这类工作流，但不会假装自己是全自动 orchestrator

### 成本与用量控制

- 用量仪表盘帮助你按 provider 跟踪请求数、Token 和成本
- 可以把贵模型留给高价值判断，把实现任务下放给更便宜的模型
- 支持自定义模型定价，便于对比不同 plan 的真实使用成本

### 配置同步与隔离

- 提供云同步、自定义配置目录、自动备份和原子写入
- 当前 fork 使用独立配置目录、独立 deep link scheme、独立本地存储前缀
- 更适合与上游版本并存安装，降低配置互相污染的风险

### MCP、Prompts 与 Skills 支撑

- 统一管理 MCP、Prompts、Skills，而不是在不同 CLI 之间重复维护
- Prompts 支持跨应用同步和回填保护
- Skills 支持从 GitHub 仓库或 ZIP 导入，适合把常用工作流沉淀下来

## 界面预览

|                  主界面                   |                  添加供应商                  |
| :---------------------------------------: | :------------------------------------------: |
| ![主界面](assets/screenshots/main-zh.png) | ![添加供应商](assets/screenshots/add-zh.png) |

## 快速开始

1. 添加你常用的 provider，比如阿里云、智谱、官方 Claude / Codex 或其他 coding plan
2. 先确定一个当前 provider 作为主入口
3. 需要更强模型做判断时，切到更强的 provider；需要更低成本执行时，把子任务 dispatch 给更便宜的 Claude / Codex provider
4. 根据用量和成本面板，逐步形成适合自己的模型分工方式

> **注意**：首次启动可以手动导入现有 CLI 配置作为默认 provider。当前分发链路主要覆盖 Claude 和 Codex provider。

## 怎么使用分发 Skill

当前分发能力是通过 **Claude Code 里的内置 skill `/dispatch-task`** 来使用的。也就是说：

- CCswitch Pro 负责管理 provider 和启动本地 dispatch 服务
- Claude Code 里通过 `/dispatch-task` 发起子任务
- 子任务目标目前支持 **Claude provider** 和 **Codex provider**

### 推荐使用流程

1. 在 CCswitch Pro 里先把 Claude / Codex provider 配好
2. 打开 CCswitch Pro，确保桌面应用处于运行状态
3. 在 Claude Code 里先执行 `/dispatch-task providers` 看当前可分发目标
4. 选一个 target，把任务 dispatch 出去
5. 用 `/dispatch-task status`、`last`、`logs` 查看执行情况

### 命令格式

```text
/dispatch-task providers [app]
/dispatch-task status
/dispatch-task last
/dispatch-task logs [count]
/dispatch-task <app:provider> [timeout=<seconds>] -- <task text>
```

说明：

- `app` 当前支持 `claude` 和 `codex`
- `provider` 可以写 `alias`、provider 名、provider id，或者 `current`
- 默认超时是 `120` 秒，最大 `900` 秒
- 真正的任务内容必须写在 `--` 后面

### 常用命令

#### 1. 查看可分发目标

```text
/dispatch-task providers
/dispatch-task providers claude
/dispatch-task providers codex
```

这个命令会列出当前能 dispatch 的 target，例如：

- `claude:current`
- `claude:opus`
- `codex:aliyun-codex`
- `codex:zhipu-codex`

如果你不确定 target 怎么写，先跑这条。

#### 2. 查看当前状态

```text
/dispatch-task status
```

适合看当前有没有任务在跑、最近一次 dispatch 的 target 是谁、耗时多久。

#### 3. 查看最近一次结果

```text
/dispatch-task last
```

适合直接回看最近一次分发任务的 callback、总结和 deliverable。

#### 4. 查看最近几次历史

```text
/dispatch-task logs
/dispatch-task logs 5
```

适合快速看最近几次 dispatch 是否成功、分别发给了谁、有没有超时。

#### 5. 真正发起一个子任务

```text
/dispatch-task claude:current -- 先做架构分析，给出拆解方案和实施顺序。
/dispatch-task claude:opus -- 先评估这个需求的边界、风险和模块划分。
/dispatch-task codex:current -- 根据已有方案直接实现第一部分代码。
/dispatch-task codex:aliyun-codex timeout=600 -- 按照上面的拆解完成 API 层和测试。
```

### 一个典型工作流例子

#### Opus 做架构，Codex 做实现

1. 先看目标列表

```text
/dispatch-task providers
```

2. 让更强的 Claude provider 做架构拆解

```text
/dispatch-task claude:opus -- 先阅读当前项目结构，输出这个需求的模块拆分、风险点和推荐实现顺序。
```

3. 再让 Codex provider 按拆解去实现

```text
/dispatch-task codex:aliyun-codex -- 按刚才的拆解先实现第一阶段，修改代码并补上必要测试。
```

4. 用状态和历史回看结果

```text
/dispatch-task status
/dispatch-task last
/dispatch-task logs 5
```

#### 多个 coding plan 轮流用

如果你同时有阿里云和智谱的 coding plan，可以先把两边都导入 CCswitch Pro，然后在 Claude Code 里按任务特点切换：

```text
/dispatch-task codex:aliyun-codex -- 先处理大批量实现任务。
/dispatch-task codex:zhipu-codex -- 再处理另一个更适合当前模型的子任务。
```

### 使用时的几个要点

- 不确定目标写法时，先执行 `/dispatch-task providers`
- 想用当前激活中的 provider，可以直接写 `claude:current` 或 `codex:current`
- 任务太长时，可以显式加 `timeout=600`
- 如果提示找不到 dispatch service，先确认 **CCswitch Pro 桌面应用正在运行**

## 安装

### 系统要求

- **Windows**：Windows 10 及以上
- **macOS**：macOS 12 (Monterey) 及以上
- **Linux**：Ubuntu 22.04+ / Debian 11+ / Fedora 34+ 等主流发行版

### 安装方式

从 [Releases](../../releases) 页面下载对应平台的构建产物即可。

- **Windows**：MSI 安装包或便携版 ZIP
- **macOS**：对应发布页提供的应用压缩包
- **Linux**：`.deb`、`.rpm`、`.AppImage` 或 `.flatpak` 等构建产物

> **注意**：当前以手动下载安装为主。像 Homebrew、系统仓库这类包管理发布方式，建议等这个 fork 的独立发布流程稳定后再补上。

## 常见问题

<details>
<summary><strong>CCswitch Pro 适合哪些工作流？</strong></summary>

它最适合两类工作流：一类是你手上有多个 coding plan / provider，希望根据价格、稳定性、模型能力随时切换；另一类是你希望让强模型做架构和判断，让更便宜的模型做实现和执行。

</details>

<details>
<summary><strong>能不能让 Opus 做架构，Codex 做实现？</strong></summary>

可以把它用成这种工作流。CCswitch Pro 支持管理多个 provider，并把子任务 dispatch 给指定的 Claude 或 Codex provider。但它不是“自动让 Opus 调度 Codex”的全自动编排器，更多是给你搭好这种协作链路。

</details>

<details>
<summary><strong>它是不是一个全自动多模型 orchestrator？</strong></summary>

不是。它更像是一个把多个 provider、多个 CLI 工具、以及 Claude / Codex 分发链路组织起来的工作台，帮助你更容易搭建多模型协作，而不是替你自动完成全部编排决策。

</details>

<details>
<summary><strong>切换 provider 后需要重启终端吗？</strong></summary>

大多数工具需要重启终端或 CLI 工具才能生效。例外的是 **Claude Code**，目前支持热切换 provider 数据，无需重启。

</details>

<details>
<summary><strong>我的数据存在哪里？</strong></summary>

- **数据库**：`~/.ccswitch-pro/cc-switch.db`
- **本地设置**：`~/.ccswitch-pro/settings.json`
- **备份**：`~/.ccswitch-pro/backups/`
- **Skills**：`~/.ccswitch-pro/skills/`
- **技能备份**：`~/.ccswitch-pro/skill-backups/`

当前 fork 还使用独立的 deep link scheme `ccswitchpro://`，并把本地存储命名空间与上游隔开，方便并存安装。

</details>

<details>
<summary><strong>为什么不是直接用上游版？</strong></summary>

上游已经提供了成熟的 provider 管理基础，但这个 fork 更强调多 provider 协作工作流、分发链路和独立实例隔离。如果你的核心诉求就是“把多个订阅和不同模型更顺手地组织起来”，这个版本会更贴近这个方向。

</details>

## 文档

如需查看更细的功能说明，请查阅 **[用户手册](docs/user-manual/zh/README.md)**，包含 provider 管理、MCP、Prompts、Skills、代理与故障转移等说明。

## Fork 来源

**CCswitch Pro** 基于 [CC Switch](https://github.com/farion1231/cc-switch) 持续维护。

这个 fork 保留了上游成熟的基础能力，同时补上了更偏向多模型协作场景的能力与隔离细节，包括独立 app identity、独立配置目录、独立 WebDAV 根目录、独立 deep link scheme、独立本地存储前缀，以及更顺手的 Claude / Codex 分发目标选择。

<details>
<summary><strong>架构总览</strong></summary>

### 设计原则

```
┌─────────────────────────────────────────────────────────────┐
│                    前端 (React + TS)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   （UI）     │──│ （业务逻辑）   │──│   （缓存/同步）    │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────────┐
│                  后端 (Tauri + Rust)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │  Models/Config   │    │
│  │ （API 层）   │──│  （业务层）    │──│    （数据）       │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**核心设计模式**

- **SSOT**（单一事实源）：所有数据存储在 `~/.ccswitch-pro/cc-switch.db`（SQLite）
- **双层存储**：SQLite 存储可同步数据，JSON 存储设备级设置
- **双向同步**：切换时写入 live 文件，编辑当前供应商时从 live 回填
- **原子写入**：临时文件 + 重命名模式防止配置损坏
- **并发安全**：Mutex 保护的数据库连接避免竞态条件
- **分层架构**：清晰分离（Commands → Services → DAO → Database）

**核心组件**

- **ProviderService**：供应商增删改查、切换、回填、排序
- **McpService**：MCP 服务器管理、导入导出、live 文件同步
- **ProxyService**：本地 Proxy 模式，支持热切换和格式转换
- **SessionManager**：Claude Code 对话历史浏览
- **ConfigService**：配置导入导出、备份轮换
- **SpeedtestService**：API 端点延迟测量

</details>

<details>
<summary><strong>开发指南</strong></summary>

### 环境要求

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### 开发命令

```bash
# 安装依赖
pnpm install

# 开发模式（热重载）
pnpm dev

# 类型检查
pnpm typecheck

# 代码格式化
pnpm format

# 检查代码格式
pnpm format:check

# 运行前端单元测试
pnpm test:unit

# 监听模式运行测试（推荐开发时使用）
pnpm test:unit:watch

# 构建应用
pnpm build

# 构建调试版本
pnpm tauri build --debug
```

### Rust 后端开发

```bash
cd src-tauri

# 格式化 Rust 代码
cargo fmt

# 运行 clippy 检查
cargo clippy

# 运行后端测试
cargo test

# 运行特定测试
cargo test test_name

# 运行带测试 hooks 的测试
cargo test --features test-hooks
```

### 测试说明

**前端测试**：

- 使用 **vitest** 作为测试框架
- 使用 **MSW (Mock Service Worker)** 模拟 Tauri API 调用
- 使用 **@testing-library/react** 进行组件测试

**运行测试**：

```bash
# 运行所有测试
pnpm test:unit

# 监听模式（自动重跑）
pnpm test:unit:watch

# 带覆盖率报告
pnpm test:unit --coverage
```

### 技术栈

**前端**：React 18 · TypeScript · Vite · TailwindCSS 3.4 · TanStack Query v5 · react-i18next · react-hook-form · zod · shadcn/ui · @dnd-kit

**后端**：Tauri 2.8 · Rust · serde · tokio · thiserror · tauri-plugin-updater/process/dialog/store/log

**测试**：vitest · MSW · @testing-library/react

</details>

<details>
<summary><strong>项目结构</strong></summary>

```
├── src/                        # 前端 (React + TypeScript)
│   ├── components/
│   │   ├── providers/          # 供应商管理
│   │   ├── mcp/                # MCP 面板
│   │   ├── prompts/            # Prompts 管理
│   │   ├── skills/             # Skills 管理
│   │   ├── sessions/           # 会话管理器
│   │   ├── proxy/              # Proxy 模式面板
│   │   ├── openclaw/           # OpenClaw 配置面板
│   │   ├── settings/           # 设置（终端/备份/关于）
│   │   ├── deeplink/           # Deep Link 导入
│   │   ├── env/                # 环境变量管理
│   │   ├── universal/          # 跨应用配置
│   │   ├── usage/              # 用量统计
│   │   └── ui/                 # shadcn/ui 组件库
│   ├── hooks/                  # 自定义 hooks（业务逻辑）
│   ├── lib/
│   │   ├── api/                # Tauri API 封装（类型安全）
│   │   └── query/              # TanStack Query 配置
│   ├── locales/                # 翻译 (zh/en/ja)
│   ├── config/                 # 预设 (providers/mcp)
│   └── types/                  # TypeScript 类型定义
├── src-tauri/                  # 后端 (Rust)
│   └── src/
│       ├── commands/           # Tauri 命令层（按领域）
│       ├── services/           # 业务逻辑层
│       ├── database/           # SQLite DAO 层
│       ├── proxy/              # Proxy 模块
│       ├── session_manager/    # 会话管理
│       ├── deeplink/           # Deep Link 处理
│       └── mcp/                # MCP 同步模块
├── tests/                      # 前端测试
└── assets/                     # 截图 & 合作商资源
```

</details>

## 贡献

欢迎提交 Issue 反馈问题和建议！

提交 PR 前请确保：

- 通过类型检查：`pnpm typecheck`
- 通过格式检查：`pnpm format:check`
- 通过单元测试：`pnpm test:unit`

新功能开发前，欢迎先开 Issue 讨论实现方案，不适合项目的功能性 PR 有可能会被关闭。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=notice501/cc-switch&type=Date)](https://www.star-history.com/#notice501/cc-switch&Date)

## License

MIT © Jason Young
