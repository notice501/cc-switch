use crate::task_dispatcher::{TaskDispatcher, TaskRequest, TaskType, TaskPriority};
use crate::app_config::MultiAppConfig;

/// 任务分派系统功能演示和测试
#[tokio::test]
async fn demo_task_dispatcher() {
    // 创建任务分派器
    let config = MultiAppConfig::default();
    let dispatcher = TaskDispatcher::new(config);

    // 创建一个代码生成任务
    let code_task = TaskRequest {
        id: "demo_code_task_1".to_string(),
        task_type: TaskType::CodeGeneration,
        priority: TaskPriority::High,
        content: "Create a simple Rust function that adds two numbers".to_string(),
        target_platform: None, // 自动选择平台
        submitted_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        timeout_seconds: Some(30),
    };

    // 提交任务
    let task_id = dispatcher.submit_task(code_task.clone()).await
        .expect("Failed to submit task");
    println!("Submitted task with ID: {}", task_id);

    // 检查队列长度
    let queue_len = dispatcher.get_queue_length().await;
    assert_eq!(queue_len, 1);
    println!("Queue length: {}", queue_len);

    // 执行任务
    let executed_task_id = dispatcher.execute_next_task().await
        .expect("Failed to execute task")
        .expect("No task was executed");
    println!("Executed task with ID: {}", executed_task_id);

    // 检查任务状态
    let status = dispatcher.get_task_status(&executed_task_id).await
        .expect("Failed to get task status");
    println!("Task status: {:?}", status);

    // 创建另一个任务，指定目标平台
    let math_task = TaskRequest {
        id: "demo_math_task_1".to_string(),
        task_type: TaskType::MathCalculation,
        priority: TaskPriority::Normal,
        content: "Calculate the derivative of x^3 + 2x^2 + x + 1".to_string(),
        target_platform: Some(crate::app_config::AppType::Codex), // 指定使用 Codex
        submitted_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        timeout_seconds: Some(30),
    };

    // 提交第二个任务
    let task_id2 = dispatcher.submit_task(math_task).await
        .expect("Failed to submit math task");
    println!("Submitted math task with ID: {}", task_id2);

    // 执行第二个任务
    let executed_task_id2 = dispatcher.execute_next_task().await
        .expect("Failed to execute math task")
        .expect("No math task was executed");
    println!("Executed math task with ID: {}", executed_task_id2);

    // 获取队列状态
    let queue_status = dispatcher.get_queue_length().await;
    println!("Remaining tasks in queue: {}", queue_status);

    // 获取历史记录
    let history = dispatcher.get_task_history().await;
    println!("Total completed tasks: {}", history.len());

    assert_eq!(history.len(), 2);
    println!("Task dispatcher demo completed successfully!");
}

#[tokio::main]
async fn main() {
    println!("Starting task dispatcher demo...\n");

    // 运行演示
    demo_task_dispatcher().await;

    println!("\nDemo finished!");
}