# Task Dispatcher Skill

这是一个用于将任务分派到不同AI模型执行的技能。您可以使用此技能将特定任务委派给 Claude、Codex、Gemini 或其他支持的 AI 模型。

## 安装

将整个 `task-dispatcher` 目录放置在您的 Claude Code 技能目录中。

## 使用方法

在 Claude Code 中，您可以使用以下命令：

```
/dispatch-task --target codex --task "请帮我写一个 Rust 函数来实现快速排序算法"
```

或者：

```
/dispatch-task --target gemini --task "请帮我分析这段代码的性能瓶颈" --timeout 60
```

## 参数说明

- `--target` (必需): 目标 AI 模型
  - `claude`: 使用 Claude 模型
  - `codex`: 使用 Codex 模型
  - `gemini`: 使用 Gemini 模型
  - `opencode`: 使用 OpenCode 模型

- `--task` (必需): 要执行的任务描述

- `--timeout` (可选): 超时时间（秒），默认为 30

## 工作原理

此技能利用 ccswitch 的后端能力，将任务分派到指定的 AI 模型进行处理。它使用了您之前实现的任务分派系统，该系统可以智能地路由任务到最适合的 AI 平台。

## 示例用例

1. **代码生成任务**: 使用 `--target codex` 来获得更好的代码生成结果
2. **数学计算任务**: 使用 `--target codex` 来处理复杂的数学问题
3. **创意写作任务**: 使用 `--target gemini` 来获得更具创造性的回应
4. **深度分析任务**: 使用 `--target claude` 来获得更深入的分析

## 注意事项

- 确保 ccswitch 应用正在运行并且配置正确
- 不同的 AI 模型可能有不同的响应时间和能力
- 使用适当的超时值以避免长时间等待