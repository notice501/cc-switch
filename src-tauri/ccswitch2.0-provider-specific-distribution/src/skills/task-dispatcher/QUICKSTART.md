# 快速开始：Task Dispatcher 技能

## 简介
这个技能让您可以在 Claude Code 中直接将任务分派给不同的 AI 模型（如 Codex、Gemini 等）执行。

## 基本用法
```
/dispatch-task --target codex --task "请帮我写一个 Rust 函数"
```

## 支持的目标模型
- `claude` - 适合深度分析和推理
- `codex` - 适合代码生成和数学计算
- `gemini` - 适合创意写作和内容生成
- `opencode` - 适合开源项目相关任务

## 实际例子
在 Claude Code 中尝试这些命令：

1. 代码生成：
```
/dispatch-task --target codex --task "写一个快速排序的 Python 实现"
```

2. 深度分析：
```
/dispatch-task --target claude --task "分析这段代码的安全隐患"
```

3. 创意写作：
```
/dispatch-task --target gemini --task "为技术博客写一个吸引人的标题"
```

就是这样！任务会被智能地路由到最适合的 AI 模型进行处理。