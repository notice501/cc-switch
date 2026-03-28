//! 任务分派相关的 Tauri 命令

use crate::task_dispatcher::{TaskDispatcher, TaskRequest, TaskType, TaskPriority};
use serde::{Deserialize, Serialize};
use tauri::State;
use std::sync::Arc;

// 任务分派器状态
pub type TaskDispatcherState = State<'static, Arc<TaskDispatcher>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitTaskRequest {
    pub task_type: String,  // "code_generation", "text_analysis", "math_calculation", "creative_writing", "translation", "custom:xxx"
    pub priority: String,   // "low", "normal", "high", "critical"
    pub content: String,
    pub target_provider_id: String, // 必须指定具体供应商ID
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitTaskResponse {
    pub task_id: String,
    pub message: String,
    pub assigned_to_provider: String,
    pub assigned_to_platform: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusResponse {
    pub task_id: String,
    pub status: String,  // "pending", "running", "completed", "failed", "cancelled", "timeout"
    pub result: Option<String>,
    pub executed_on: String,             // 执行的供应商
    pub executed_on_platform: Option<String>, // 执行的平台
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatusResponse {
    pub pending_tasks: usize,
    pub active_tasks: usize,
    pub history_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableProvidersResponse {
    pub providers: Vec<ProviderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub platform: String,
}

/// 提交任务到分派器
#[tauri::command]
pub async fn submit_task(
    dispatcher: TaskDispatcherState,
    request: SubmitTaskRequest,
) -> Result<SubmitTaskResponse, String> {
    // 转换请求参数
    let task_type = match request.task_type.as_str() {
        "code_generation" => TaskType::CodeGeneration,
        "text_analysis" => TaskType::TextAnalysis,
        "math_calculation" => TaskType::MathCalculation,
        "creative_writing" => TaskType::CreativeWriting,
        "translation" => TaskType::Translation,
        custom if custom.starts_with("custom:") => {
            TaskType::Custom(custom.strip_prefix("custom:").unwrap().to_string())
        }
        _ => return Err(format!("Invalid task type: {}", request.task_type)),
    };

    let priority = match request.priority.as_str() {
        "low" => TaskPriority::Low,
        "normal" => TaskPriority::Normal,
        "high" => TaskPriority::High,
        "critical" => TaskPriority::Critical,
        _ => return Err(format!("Invalid priority: {}", request.priority)),
    };

    // Store the target provider ID for later use before it's moved
    let target_provider_id_clone = request.target_provider_id.clone();

    let task = TaskRequest {
        id: format!("task_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()),
        task_type,
        priority,
        content: request.content,
        target_provider_id: request.target_provider_id,
        submitted_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        timeout_seconds: request.timeout_seconds,
    };

    let task_id = dispatcher.submit_task(task).await?;

    // 获取目标供应商的平台信息
    let platform = dispatcher.find_provider_platform(&target_provider_id_clone).await;

    Ok(SubmitTaskResponse {
        task_id,
        message: format!("Task submitted successfully to provider '{}'", target_provider_id_clone),
        assigned_to_provider: target_provider_id_clone,
        assigned_to_platform: platform.map(|ap| ap.as_str().to_string()),
    })
}

/// 获取任务状态
#[tauri::command]
pub async fn get_task_status(
    dispatcher: TaskDispatcherState,
    task_id: String,
) -> Result<TaskStatusResponse, String> {
    use crate::task_dispatcher::TaskResult;

    let response = match dispatcher.get_task_status(&task_id).await {
        Some(task_response) => {
            let status = match &task_response.result {
                TaskResult::Success(_) => "completed",
                TaskResult::Failure(_) => "failed",
                TaskResult::Timeout => "timeout",
                TaskResult::Cancelled => "cancelled",
            }.to_string();

            let result = match task_response.result {
                TaskResult::Success(content) => Some(content),
                TaskResult::Failure(error) => Some(error),
                _ => None,
            };

            TaskStatusResponse {
                task_id: task_response.id,
                status,
                result,
                executed_on: task_response.executed_on,
                executed_on_platform: task_response.executed_on_platform.map(|ap| ap.as_str().to_string()),
                completed_at: Some(task_response.completed_at),
            }
        }
        None => {
            // 检查是否在队列中或活跃任务中
            TaskStatusResponse {
                task_id,
                status: "pending".to_string(),
                result: None,
                executed_on: "".to_string(),
                executed_on_platform: None,
                completed_at: None,
            }
        }
    };

    Ok(response)
}

/// 获取队列状态
#[tauri::command]
pub async fn get_queue_status(
    dispatcher: TaskDispatcherState,
) -> Result<QueueStatusResponse, String> {
    let pending_tasks = dispatcher.get_queue_length().await;
    let active_tasks = dispatcher.get_active_task_count().await;
    let history_count = dispatcher.get_task_history().await.len();

    Ok(QueueStatusResponse {
        pending_tasks,
        active_tasks,
        history_count,
    })
}

/// 获取所有可用供应商
#[tauri::command]
pub async fn get_available_providers(
    dispatcher: TaskDispatcherState,
) -> Result<AvailableProvidersResponse, String> {
    let providers = dispatcher.get_available_providers().await;

    let providers_info = providers
        .into_iter()
        .map(|(id, display_name)| {
            // 解析显示名称以提取平台信息
            let parts: Vec<&str> = display_name.split(" (").collect();
            let name = parts[0].to_string();
            let platform = if parts.len() > 1 {
                parts[1].trim_end_matches(')').to_string()
            } else {
                "unknown".to_string()
            };

            ProviderInfo {
                id,
                name,
                platform,
            }
        })
        .collect();

    Ok(AvailableProvidersResponse {
        providers: providers_info,
    })
}

/// 执行下一个任务（主要用于测试）
#[tauri::command]
pub async fn execute_next_task(
    dispatcher: TaskDispatcherState,
) -> Result<Option<String>, String> {
    dispatcher.execute_next_task().await
}

/// 获取特定供应商的信息
#[tauri::command]
pub async fn get_provider_info(
    dispatcher: TaskDispatcherState,
    provider_id: String,
) -> Result<ProviderInfo, String> {
    let providers = dispatcher.get_available_providers().await;

    for (id, display_name) in providers {
        if id == provider_id {
            let parts: Vec<&str> = display_name.split(" (").collect();
            let name = parts[0].to_string();
            let platform = if parts.len() > 1 {
                parts[1].trim_end_matches(')').to_string()
            } else {
                "unknown".to_string()
            };

            return Ok(ProviderInfo {
                id,
                name,
                platform,
            });
        }
    }

    Err(format!("Provider '{}' not found", provider_id))
}