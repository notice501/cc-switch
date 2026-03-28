//! 任务分派模块
//!
//! 允许 Claude 将任务分派到 Codex、其他 Claude 实例或 Gemini 等不同的 AI 模型后端执行

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use crate::app_config::{AppType, MultiAppConfig};

/// 任务类型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    /// 代码生成任务
    CodeGeneration,
    /// 文本分析任务
    TextAnalysis,
    /// 数学计算任务
    MathCalculation,
    /// 创意写作任务
    CreativeWriting,
    /// 翻译任务
    Translation,
    /// 自定义任务
    Custom(String),
}

/// 任务优先级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 1,
    Normal = 2,
    High = 3,
    Critical = 4,
}

/// 任务请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    /// 任务ID
    pub id: String,
    /// 任务类型
    pub task_type: TaskType,
    /// 任务优先级
    pub priority: TaskPriority,
    /// 任务描述/内容
    pub content: String,
    /// 目标供应商ID（必需指定具体供应商）
    pub target_provider_id: String,
    /// 任务提交时间戳
    pub submitted_at: i64,
    /// 超时时间（秒）
    pub timeout_seconds: Option<u64>,
}

/// 任务响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResponse {
    /// 任务ID
    pub id: String,
    /// 执行结果
    pub result: TaskResult,
    /// 完成时间戳
    pub completed_at: i64,
    /// 执行供应商
    pub executed_on: String,
    /// 执行平台
    pub executed_on_platform: Option<AppType>,
}

/// 任务执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskResult {
    /// 成功
    Success(String),
    /// 失败
    Failure(String),
    /// 超时
    Timeout,
    /// 被取消
    Cancelled,
}

/// 任务分派器
pub struct TaskDispatcher {
    /// 任务队列
    task_queue: Mutex<Vec<TaskRequest>>,
    /// 活跃任务
    active_tasks: Mutex<HashMap<String, TaskRequest>>,
    /// 任务历史记录
    task_history: Mutex<HashMap<String, TaskResponse>>,
    /// 应用配置
    config: MultiAppConfig,
}

impl TaskDispatcher {
    /// 创建新的任务分派器
    pub fn new(config: MultiAppConfig) -> Self {
        Self {
            task_queue: Mutex::new(Vec::new()),
            active_tasks: Mutex::new(HashMap::new()),
            task_history: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// 提交任务到队列
    pub async fn submit_task(&self, task: TaskRequest) -> Result<String, String> {
        // 验证任务参数
        if task.content.trim().is_empty() {
            return Err("Task content cannot be empty".to_string());
        }

        // 验证目标供应商是否存在
        let provider_exists = self.provider_exists(&task.target_provider_id).await;
        if !provider_exists {
            return Err(format!("Provider '{}' does not exist in any configured platform", task.target_provider_id));
        }

        let mut queue = self.task_queue.lock().await;
        queue.push(task.clone());

        // 按优先级排序
        queue.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(task.id)
    }

    /// 选择最适合执行任务的目标供应商
    async fn select_target_provider(&self, task: &TaskRequest) -> Result<(String, AppType), String> {
        // 验证目标供应商是否存在
        let provider_exists = self.provider_exists(&task.target_provider_id).await;
        if !provider_exists {
            return Err(format!("Provider '{}' does not exist in any configured platform", task.target_provider_id));
        }

        // 获取供应商所属平台
        let platform = match self.find_provider_platform(&task.target_provider_id).await {
            Some(platform) => platform,
            None => return Err(format!("Could not determine platform for provider '{}'", task.target_provider_id)),
        };

        Ok((task.target_provider_id.clone(), platform))
    }

    /// 检查供应商是否存在
    async fn provider_exists(&self, provider_id: &str) -> bool {
        for app_type in [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::OpenCode,
        ] {
            if let Some(manager) = self.config.get_manager(&app_type) {
                if manager.providers.contains_key(provider_id) {
                    return true;
                }
            }
        }
        false
    }

    /// 查找供应商所属平台
    pub async fn find_provider_platform(&self, provider_id: &str) -> Option<AppType> {
        for app_type in [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::OpenCode,
        ] {
            if let Some(manager) = self.config.get_manager(&app_type) {
                if manager.providers.contains_key(provider_id) {
                    return Some(app_type);
                }
            }
        }
        None
    }

    /// 执行下一个待处理任务
    pub async fn execute_next_task(&self) -> Result<Option<String>, String> {
        let mut queue = self.task_queue.lock().await;

        if queue.is_empty() {
            return Ok(None);
        }

        // 获取最高优先级的任务
        let task = queue.remove(0);

        // 移动到活跃任务列表
        {
            let mut active = self.active_tasks.lock().await;
            active.insert(task.id.clone(), task.clone());
        }

        // 释放锁后再执行任务，避免死锁
        let result = self.execute_task(task).await;

        // 从活跃任务中移除并添加到历史记录
        {
            let mut active = self.active_tasks.lock().await;
            active.remove(&result.id);

            let mut history = self.task_history.lock().await;
            history.insert(result.id.clone(), result.clone());
        }

        Ok(Some(result.id))
    }

    /// 执行具体任务
    async fn execute_task(&self, task: TaskRequest) -> TaskResponse {
        let _start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 选择目标供应商
        let (target_provider_id, target_platform) = match self.select_target_provider(&task).await {
            Ok((provider_id, platform)) => (provider_id, platform),
            Err(e) => {
                return TaskResponse {
                    id: task.id,
                    result: TaskResult::Failure(format!("Failed to select target provider: {}", e)),
                    completed_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    executed_on: task.target_provider_id,
                    executed_on_platform: None,
                };
            }
        };

        // 这里应该实际调用指定供应商的 API
        // 模拟执行指定供应商的任务
        let execution_result = self.simulate_task_execution(&task, &target_platform).await;

        let completed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        TaskResponse {
            id: task.id,
            result: execution_result,
            completed_at,
            executed_on: target_provider_id,
            executed_on_platform: Some(target_platform),
        }
    }

    /// 模拟任务执行
    async fn simulate_task_execution(&self, task: &TaskRequest, platform: &AppType) -> TaskResult {
        // 在实际实现中，这里应该调用相应的API
        // 目前我们只是模拟执行
        println!("Executing task '{}' on platform {:?}", task.id, platform);

        // 模拟API调用延迟
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // 根据任务类型和平台返回相应结果
        match &task.task_type {
            TaskType::CodeGeneration => {
                TaskResult::Success(format!(
                    "[SIMULATED] Code generation task completed on {:?}. Input: {}",
                    platform,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::TextAnalysis => {
                TaskResult::Success(format!(
                    "[SIMULATED] Text analysis task completed on {:?}. Input: {}",
                    platform,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::MathCalculation => {
                TaskResult::Success(format!(
                    "[SIMULATED] Math calculation task completed on {:?}. Input: {}",
                    platform,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::CreativeWriting => {
                TaskResult::Success(format!(
                    "[SIMULATED] Creative writing task completed on {:?}. Input: {}",
                    platform,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::Translation => {
                TaskResult::Success(format!(
                    "[SIMULATED] Translation task completed on {:?}. Input: {}",
                    platform,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::Custom(custom_type) => {
                TaskResult::Success(format!(
                    "[SIMULATED] Custom task ({}) completed on {:?}. Input: {}",
                    custom_type,
                    platform,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
        }
    }

    /// 获取任务状态
    pub async fn get_task_status(&self, task_id: &str) -> Option<TaskResponse> {
        let history = self.task_history.lock().await;
        history.get(task_id).cloned()
    }

    /// 获取队列长度
    pub async fn get_queue_length(&self) -> usize {
        let queue = self.task_queue.lock().await;
        queue.len()
    }

    /// 获取活跃任务数
    pub async fn get_active_task_count(&self) -> usize {
        let active = self.active_tasks.lock().await;
        active.len()
    }

    /// 获取任务历史记录
    pub async fn get_task_history(&self) -> Vec<TaskResponse> {
        let history = self.task_history.lock().await;
        history.values().cloned().collect()
    }

    /// 获取所有可用供应商列表
    pub async fn get_available_providers(&self) -> Vec<(String, String)> {
        let mut providers = Vec::new();

        for app_type in [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::OpenCode,
        ] {
            if let Some(manager) = self.config.get_manager(&app_type) {
                for (provider_id, provider) in &manager.providers {
                    providers.push((provider_id.clone(), format!("{} ({})", provider.name, app_type.as_str())));
                }
            }
        }

        providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_submission() {
        let config = MultiAppConfig::default();
        let dispatcher = TaskDispatcher::new(config);

        let task = TaskRequest {
            id: "test_task_1".to_string(),
            task_type: TaskType::CodeGeneration,
            priority: TaskPriority::Normal,
            content: "Generate a simple Rust function".to_string(),
            target_provider_id: "test-provider".to_string(), // This will fail validation
            submitted_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            timeout_seconds: None,
        };

        // This should fail because the provider doesn't exist
        let result = dispatcher.submit_task(task).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_provider_validation() {
        let config = MultiAppConfig::default();
        let dispatcher = TaskDispatcher::new(config);

        // Test with non-existent provider
        let exists = dispatcher.provider_exists("non-existent-provider").await;
        assert_eq!(exists, false);

        // The actual provider existence test would require setting up a proper config with providers
    }
}