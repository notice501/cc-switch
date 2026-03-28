# Task Dispatcher 技能使用指南

## 简介

Task Dispatcher 是一个为 Claude Code 设计的技能，允许您将任务分派给不同的 AI 模型执行，如 Codex、Gemini 等。这个技能利用 ccswitch 的后端能力来智能地路由任务到最适合的 AI 平台。

## 功能特点

- **跨平台任务分派**: 将任务发送到 Claude、Codex、Gemini 或 OpenCode
- **智能路由**: 根据任务类型自动选择最适合的 AI 模型
- **状态追踪**: 提供任务 ID 用于追踪执行状态
- **灵活配置**: 支持自定义超时和优先级设置

## 安装方法

1. 确保您已安装 ccswitch 应用并正常运行
2. 将整个 `task-dispatcher` 目录复制到 Claude Code 的技能目录中
3. 确保 `main.py` 具有执行权限

## 使用方法

在 Claude Code 的终端或聊天界面中，使用以下命令格式：

### 基本命令格式
```
/dispatch-task --target <ai-model> --task "<task-description>"
```

### 参数说明

- `--target` (必需): 目标 AI 模型
  - `claude`: 使用 Claude 模型（适合深度分析、推理）
  - `codex`: 使用 Codex 模型（适合代码生成、数学计算）
  - `gemini`: 使用 Gemini 模型（适合创意写作、多模态任务）
  - `opencode`: 使用 OpenCode 模型

- `--task` (必需): 要执行的具体任务描述

- `--timeout` (可选): 超时时间（秒），默认为 30

## 实际使用示例

### 示例 1: 代码生成任务
```
/dispatch-task --target codex --task "请帮我写一个 Rust 函数来实现二分查找算法，要求处理整数数组并返回索引"
```

### 示例 2: 深度分析任务
```
/dispatch-task --target claude --task "请分析下面这段 Python 代码的潜在性能问题和安全漏洞"
```

### 示例 3: 创意写作任务
```
/dispatch-task --target gemini --task "请为我的技术博客写一篇关于 AI 伦理的文章开头，约 200 字"
```

### 示例 4: 数学计算任务
```
/dispatch-task --target codex --task "求解方程组: 2x + 3y = 7 和 x - y = 1" --timeout 60
```

## 何时使用哪个模型

### Claude
- 适合复杂推理和分析
- 深度问题解答
- 代码审查和架构设计
- 伦理和哲学讨论

### Codex
- 代码生成和优化
- 算法实现
- 数学和逻辑问题
- 技术文档编写

### Gemini
- 创意写作
- 内容生成
- 多语言翻译
- 概念解释

### OpenCode
- 开源项目相关任务
- 特定编程语言专长
- 社区问题解答

## 返回结果解读

执行命令后，您会收到类似这样的响应：

```json
{
  "status": "submitted",
  "task_id": "dispatch_1234567890_1234",
  "target": "codex",
  "content_preview": "请帮我写一个 Rust 函数来实现二分...",
  "message": "Task '请帮我写一个...' has been submitted to codex via ccswitch task dispatcher.",
  "estimated_completion": "Processing will begin shortly, depending on queue status.",
  "next_steps": [
    "You can track task dispatch_1234567890_1234 status through ccswitch interface",
    "Task will be processed by the codex AI model"
  ]
}
```

## 故障排除

### 问题 1: "ccswitch not found"
**解决方案**: 确保 ccswitch 应用已安装并运行

### 问题 2: 无效的目标模型
**解决方案**: 检查目标模型名称是否正确 (claude, codex, gemini, opencode)

### 问题 3: 任务提交失败
**解决方案**: 检查任务内容是否过长或包含特殊字符

## 最佳实践

1. **明确任务描述**: 提供清晰、具体的任务描述以获得更好的结果
2. **选择合适的模型**: 根据任务性质选择最适合的 AI 模型
3. **合理设置超时**: 复杂任务可以设置较长的超时时间
4. **监控队列状态**: 通过 ccswitch 界面监控任务执行状态

## 与 ccswitch 的集成

这个技能直接与 ccswitch 的任务分派系统集成，利用了您之前实现的功能：
- 任务队列管理
- 智能模型路由
- 负载均衡
- 结果追踪

## 高级技巧

您可以结合 Claude Code 的其他功能一起使用：

```
Plan: 让我先用 /dispatch-task 来生成一些代码基础
然后在 Claude Code 中进行进一步的定制和调试
```

这样可以充分发挥不同 AI 模型的优势！

## 注意事项

- 确保您的 AI 模型配置正确并在 ccswitch 中可用
- 某些复杂任务可能需要较长时间处理
- 使用适当的超时值以避免不必要的等待