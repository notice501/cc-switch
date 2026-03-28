use crate::dispatch_service;

#[tauri::command]
pub async fn get_dispatch_status() -> Result<dispatch_service::DispatchStatusSnapshot, String> {
    tauri::async_runtime::spawn_blocking(dispatch_service::read_status_snapshot)
        .await
        .map_err(|e| format!("Failed to load dispatch status: {e}"))?
        .map_err(|e| e.to_string())
}
