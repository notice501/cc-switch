# AI任务分派系统

这是一个能够让 Claude 将任务分派到 Codex、其他 Claude 实例或 Gemini 等不同 AI 模型后端执行的功能模块。

## 功能特性

- **多平台支持**: 支持 Claude、Codex、Gemini、OpenCode 等多个 AI 平台
- **任务类型**: 支持代码生成、文本分析、数学计算、创意写作、翻译等多种任务类型
- **优先级管理**: 支持任务优先级设置（低、普通、高、紧急）
- **智能路由**: 根据任务类型自动选择最适合的 AI 平台
- **队列管理**: 任务队列和负载均衡机制
- **状态跟踪**: 实时监控任务执行状态

## 架构设计

### 核心组件

1. **TaskDispatcher**: 主要的任务分派器，管理任务队列、执行状态和结果
2. **TaskRequest**: 任务请求数据结构
3. **TaskResponse**: 任务响应数据结构
4. **Tauri Commands**: 提供前端接口的命令集合

### 任务分派流程

1. 前端提交任务请求到后端
2. 系统根据任务类型和平台负载选择最优的 AI 平台
3. 任务被添加到队列中并按优先级排序
4. 系统按优先级顺序执行任务
5. 返回执行结果到前端

## API 接口

### submit_task(request: SubmitTaskRequest) -> SubmitTaskResponse

提交新任务到分派系统。

参数：
- `task_type`: 任务类型 ("code_generation", "text_analysis", "math_calculation", "creative_writing", "translation", "custom:xxx")
- `priority`: 优先级 ("low", "normal", "high", "critical")
- `content`: 任务内容
- `target_platform`: 目标平台 ("claude", "codex", "gemini", "opencode")，可选
- `timeout_seconds`: 超时时间，可选

返回：
- `task_id`: 任务ID
- `message`: 状态消息

### get_task_status(task_id: String) -> TaskStatusResponse

获取特定任务的状态。

返回：
- `task_id`: 任务ID
- `status`: 任务状态 ("pending", "running", "completed", "failed", "cancelled", "timeout")
- `result`: 执行结果（如果有）
- `executed_on`: 执行平台
- `completed_at`: 完成时间戳

### get_queue_status() -> QueueStatusResponse

获取队列状态。

返回：
- `pending_tasks`: 待处理任务数
- `active_tasks`: 活跃任务数
- `history_count`: 历史任务数

## 任务类型选择策略

- **代码生成**: 优先使用 Claude
- **数学计算**: 优先使用 Codex
- **创意写作**: 优先使用 Gemini
- **文本分析**: 根据负载动态分配
- **其他类型**: 根据负载均衡分配

## 实际应用场景

1. **多模型协同工作**: 当一个AI模型在某类任务上表现不佳时，自动切换到更适合的模型
2. **负载均衡**: 当某个AI模型繁忙时，将任务分配给其他可用的模型
3. **成本优化**: 根据API价格或性能需求，智能选择最经济高效的模型
4. **容错机制**: 当一个AI服务不可用时，自动切换到备用模型

## 前端集成

前端通过 Tauri 命令调用后端功能，提供用户友好的任务分派界面。

## 未来扩展

1. **更智能的路由算法**: 基于历史性能数据进行模型选择
2. **任务分片**: 将大型任务拆分为子任务并行处理
3. **结果对比**: 在多个模型间对比结果，选择最佳答案
4. **用户自定义路由规则**: 允许用户设置自己的模型选择偏好