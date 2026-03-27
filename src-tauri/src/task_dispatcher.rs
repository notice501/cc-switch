//! 精简版任务分派模块
//!
//! 提供基础的任务分派功能，支持向不同 AI 模型分派任务

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
    /// 目标供应商ID
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

        // 简化版本：仅检查任务是否有效
        if task.target_provider_id.trim().is_empty() {
            return Err("Target provider ID cannot be empty".to_string());
        }

        let mut queue = self.task_queue.lock().await;
        queue.push(task.clone());

        // 按优先级排序
        queue.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(task.id)
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
        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 简化版执行逻辑 - 模拟任务执行
        let execution_result = self.simulate_task_execution(&task).await;

        let completed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        TaskResponse {
            id: task.id,
            result: execution_result,
            completed_at,
            executed_on: task.target_provider_id,
            executed_on_platform: Some(AppType::Claude), // 简化返回 Claude 作为默认值
        }
    }

    /// 模拟任务执行
    async fn simulate_task_execution(&self, task: &TaskRequest) -> TaskResult {
        // 模拟API调用延迟
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // 根据任务类型返回结果
        match &task.task_type {
            TaskType::CodeGeneration => {
                TaskResult::Success(format!(
                    "[SIMULATED] Code generation task completed on provider '{}'. Input: {}",
                    task.target_provider_id,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::TextAnalysis => {
                TaskResult::Success(format!(
                    "[SIMULATED] Text analysis task completed on provider '{}'. Input: {}",
                    task.target_provider_id,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::MathCalculation => {
                TaskResult::Success(format!(
                    "[SIMULATED] Math calculation task completed on provider '{}'. Input: {}",
                    task.target_provider_id,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::CreativeWriting => {
                TaskResult::Success(format!(
                    "[SIMULATED] Creative writing task completed on provider '{}'. Input: {}",
                    task.target_provider_id,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::Translation => {
                TaskResult::Success(format!(
                    "[SIMULATED] Translation task completed on provider '{}'. Input: {}",
                    task.target_provider_id,
                    task.content.chars().take(50).collect::<String>()
                ))
            }
            TaskType::Custom(custom_type) => {
                TaskResult::Success(format!(
                    "[SIMULATED] Custom task ({}) completed on provider '{}'. Input: {}",
                    custom_type,
                    task.target_provider_id,
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

    /// 检查提供商是否存在（简化版）
    pub async fn provider_exists(&self, provider_id: &str) -> bool {
        // 在真实实现中，这会检查配置中的提供商
        // 这里简化为始终返回 true 以允许测试
        !provider_id.is_empty()
    }
}