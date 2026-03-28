use std::sync::Arc;

use tauri::State;

use crate::codex_oauth::{
    codex_oauth_status_from_settings, ensure_unique_account, persist_oauth_refresh_if_needed,
    refresh_oauth_settings_if_needed, CodexOAuthLoginManager, CodexOAuthLoginPoll,
    CodexOAuthLoginStart, CodexOAuthRefreshResponse, CodexOAuthStatus,
};
use crate::store::AppState;

pub struct CodexAuthState(pub Arc<CodexOAuthLoginManager>);

#[tauri::command(rename_all = "camelCase")]
pub fn codex_auth_start_login(
    state: State<'_, CodexAuthState>,
) -> Result<CodexOAuthLoginStart, String> {
    state.0.start_login().map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn codex_auth_poll_login(
    app_state: State<'_, AppState>,
    state: State<'_, CodexAuthState>,
    session_id: String,
    provider_id: Option<String>,
) -> Result<CodexOAuthLoginPoll, String> {
    state
        .0
        .poll_login(app_state.db.as_ref(), provider_id.as_deref(), &session_id)
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn codex_auth_get_status(settings_config: serde_json::Value) -> Result<CodexOAuthStatus, String> {
    Ok(codex_oauth_status_from_settings(&settings_config))
}

#[tauri::command(rename_all = "camelCase")]
pub fn codex_auth_refresh(
    app_state: State<'_, AppState>,
    settings_config: serde_json::Value,
    provider_id: Option<String>,
) -> Result<CodexOAuthRefreshResponse, String> {
    if let Some(provider_id) = provider_id.as_deref() {
        if let Some(provider) = app_state
            .db
            .get_provider_by_id(provider_id, "codex")
            .map_err(|e| e.to_string())?
        {
            let refreshed = persist_oauth_refresh_if_needed(app_state.db.as_ref(), &provider, true)
                .map_err(|e| e.to_string())?;
            return Ok(CodexOAuthRefreshResponse {
                status: codex_oauth_status_from_settings(&refreshed),
                settings_config: refreshed,
            });
        }
    }

    let refreshed = refresh_oauth_settings_if_needed(&settings_config, true)
        .map_err(|e| e.to_string())?;
    if let Some(account_id) = refreshed
        .get("oauth")
        .and_then(|oauth| oauth.get("accountId"))
        .and_then(|value| value.as_str())
    {
        ensure_unique_account(app_state.db.as_ref(), provider_id.as_deref(), account_id)
            .map_err(|e| e.to_string())?;
    }

    Ok(CodexOAuthRefreshResponse {
        status: codex_oauth_status_from_settings(&refreshed),
        settings_config: refreshed,
    })
}
