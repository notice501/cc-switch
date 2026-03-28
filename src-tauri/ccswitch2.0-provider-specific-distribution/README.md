# CC Switch 2.0 供应商指定版

CC Switch 2.0 供应商指定版是一个专门设计用于手动指定具体供应商ID进行任务分派的增强应用。

## 功能特性

- **精确供应商路由**: 手动指定具体供应商ID进行任务分派
- **Claude 多供应商支持**: 可在 Claude 下的不同供应商间精确路由
- **保留多模型支持**: 仍支持跨不同 AI 模型分派
- **验证机制**: 验证指定供应商是否存在
- **状态跟踪**: 实时监控任务执行状态

## 安装依赖

- Rust (1.70+)
- Cargo

## 构建步骤

```bash
cd src-tauri
cargo tauri build
```

## 使用方法

在 Claude Code 中使用以下命令进行供应商指定的任务分派：

### 指定 Claude 供应商
```
/dispatch-task --target anthropic-us-east --task "请帮我写一个 Python 函数"
```

### 指定 Codex 供应商
```
/dispatch-task --target openrouter-meta-llama --task "帮我解决这个数学问题"
```

### 指定 Gemini 供应商
```
/dispatch-task --target google-gemini-pro --task "帮我生成一段文本"
```

## 参数说明

- `--target`: 目标供应商ID（必须是您在 CC Switch 中已配置的供应商）
- `--task`: 要执行的任务描述
- `--timeout`: 超时时间（秒），可选

## 供应商验证

系统会自动验证您指定的供应商是否存在，如果供应商不存在会返回错误。

## 查看可用供应商

您可以通过以下命令查看所有已配置的供应商：
```
/get-available-providers
```

## 与现有配置集成

- 直接使用您在 CC Switch 中已配置的所有供应商
- 无需额外配置
- 完全兼容现有的供应商设置

## 示例

```
# 分派到特定的 Claude 供应商
/dispatch-task --target claude-sonnet-fast --task "帮我优化这个算法"

# 分派到特定的 OpenRouter 供应商
/dispatch-task --target openrouter-claude-3-opus --task "帮我分析这段代码"

# 分派到特定的 Azure OpenAI 供应商
/dispatch-task --target azure-gpt4-turbo --task "帮我生成报告摘要"
```

## 注意事项

- 确保指定的供应商ID在您的 CC Switch 配置中存在
- 供应商ID区分大小写
- 系统会验证供应商是否存在，不存在会返回错误