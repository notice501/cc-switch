use crate::{dispatch_service, store::AppState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOverview {
    status: dispatch_service::DispatchStatusSnapshot,
    runs: Vec<dispatch_service::DispatchRunRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlanArgs {
    task: String,
    policy: Option<String>,
    target: Option<String>,
    mode: Option<String>,
}

#[tauri::command]
pub async fn get_dispatch_status() -> Result<dispatch_service::DispatchStatusSnapshot, String> {
    tauri::async_runtime::spawn_blocking(dispatch_service::read_status_snapshot)
        .await
        .map_err(|e| format!("Failed to load dispatch status: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agent_overview(
    state: tauri::State<'_, AppState>,
) -> Result<AgentOverview, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let status = dispatch_service::read_status_snapshot()?;
        let runs = dispatch_service::read_recent_runs(&db, 8)?;
        Ok::<_, crate::error::AppError>(AgentOverview { status, runs })
    })
    .await
    .map_err(|e| format!("Failed to load agent overview: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plan_agent_route(
    state: tauri::State<'_, AppState>,
    args: AgentPlanArgs,
) -> Result<dispatch_service::AgentPlan, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        dispatch_service::plan_agent_task(
            &db,
            &args.task,
            args.policy.as_deref(),
            args.target.as_deref(),
            args.mode.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Failed to plan agent route: {e}"))?
    .map_err(|e| e.to_string())
}
