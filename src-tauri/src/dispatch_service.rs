use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{oneshot, watch, Mutex};
use uuid::Uuid;

use crate::app_config::AppType;
use crate::config::{get_app_config_dir, sanitize_provider_name};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::build_effective_settings_with_common_config;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const MAX_TIMEOUT_SECONDS: u64 = 900;
const HISTORY_FILE_NAME: &str = "dispatch-history.jsonl";
const DISCOVERY_FILE_NAME: &str = "dispatch-api.json";
const STATUS_FILE_NAME: &str = "dispatch-status.json";
const MAIN_AGENT_CALLBACK_TAG: &str = "MAIN_AGENT_CALLBACK";
const MAX_STORED_OUTPUT_CHARS: i64 = 120_000;
const AGENT_RUNTIME_BACKGROUND: &str = "background";
const AGENT_RUNTIME_INLINE: &str = "inline";
const AGENT_RUNTIME_PANE: &str = "pane";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchDiscovery {
    pub base_url: String,
    pub token: String,
    pub pid: u32,
    pub updated_at: i64,
}

#[derive(Clone)]
struct DispatchApiState {
    db: Arc<Database>,
    token: Arc<String>,
    base_url: Arc<String>,
    cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchProvidersResponse {
    providers: Vec<DispatchProviderTarget>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchRunsResponse {
    runs: Vec<DispatchRunRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentPlanResponse {
    accepted: bool,
    task: String,
    cwd: String,
    plan: AgentPlan,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunResponse {
    accepted: bool,
    completed: bool,
    run: DispatchRunResponse,
    plan: AgentPlan,
    legacy_command_hint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchCancelResponse {
    ok: bool,
    run: DispatchRunRecord,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBridgePrepareResponse {
    accepted: bool,
    completed: bool,
    run_id: String,
    target: String,
    provider_name: String,
    state: String,
    started_at: i64,
    timeout_seconds: u64,
    cwd: String,
    spec_path: String,
    callback_pane: String,
    callback_mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchProviderTarget {
    target: String,
    canonical_target: String,
    app: String,
    provider_id: String,
    provider_name: String,
    current: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchRunRequest {
    target: String,
    task: String,
    timeout_seconds: Option<u64>,
    cwd: Option<String>,
    wait_for_completion: Option<bool>,
    route_policy: Option<String>,
    task_kind: Option<String>,
    reasoning_level: Option<String>,
    runtime_mode: Option<String>,
    callback_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBridgePrepareRequest {
    target: String,
    task: String,
    timeout_seconds: Option<u64>,
    cwd: Option<String>,
    callback_pane: String,
    callback_mode: Option<String>,
    host_app: Option<String>,
    route_policy: Option<String>,
    task_kind: Option<String>,
    reasoning_level: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentPlanRequest {
    task: String,
    cwd: Option<String>,
    policy: Option<String>,
    target: Option<String>,
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunRequest {
    task: String,
    cwd: Option<String>,
    policy: Option<String>,
    target: Option<String>,
    mode: Option<String>,
    timeout_seconds: Option<u64>,
    wait_for_completion: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchRunsQuery {
    limit: Option<usize>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBridgeStartedRequest {
    pane_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBridgeCompleteRequest {
    status: String,
    timed_out: bool,
    cancelled: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    result: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchRunResponse {
    accepted: bool,
    completed: bool,
    run_id: String,
    target: String,
    provider_name: String,
    state: String,
    started_at: i64,
    timeout_seconds: u64,
    cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timed_out: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cancelled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchHistoryEntry {
    #[serde(default)]
    run_id: String,
    timestamp: i64,
    target: String,
    provider_name: String,
    cwd: String,
    timeout_seconds: u64,
    status: String,
    timed_out: bool,
    #[serde(default)]
    cancelled: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    result_preview: String,
    #[serde(default)]
    result: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchStatusSnapshot {
    state: String,
    updated_at: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    running_runs: Vec<DispatchActiveRun>,
    current_run: Option<DispatchActiveRun>,
    last_run: Option<DispatchStatusRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchActiveRun {
    #[serde(default)]
    run_id: String,
    started_at: i64,
    target: String,
    provider_name: String,
    cwd: String,
    timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchStatusRun {
    #[serde(default)]
    run_id: String,
    timestamp: i64,
    target: String,
    provider_name: String,
    cwd: String,
    timeout_seconds: u64,
    status: String,
    timed_out: bool,
    #[serde(default)]
    cancelled: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    result_preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchRunRecord {
    run_id: String,
    target: String,
    provider_name: String,
    host_app: String,
    route_policy: String,
    task_kind: String,
    reasoning_level: String,
    runtime_mode: String,
    callback_mode: String,
    cwd: String,
    task_preview: String,
    status: String,
    timeout_seconds: u64,
    started_at: i64,
    updated_at: i64,
    finished_at: Option<i64>,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    timed_out: bool,
    cancelled: bool,
    result_preview: String,
    result: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBridgeLaunchSpec {
    base_url: String,
    token: String,
    run_id: String,
    target: String,
    provider_name: String,
    cwd: String,
    timeout_seconds: u64,
    callback_pane: String,
    callback_mode: String,
    command: Vec<String>,
    env: HashMap<String, String>,
    env_remove: Vec<String>,
    path_prefix: Option<String>,
    last_message_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlan {
    route_policy: String,
    task_kind: String,
    reasoning_level: String,
    cost_tier: String,
    preferred_runtime: String,
    recommended_target: String,
    recommended_provider_name: String,
    fallback_chain: Vec<String>,
    explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTarget {
    app: AppType,
    provider_selector: String,
}

#[derive(Debug)]
struct RunnerOutput {
    status: &'static str,
    timed_out: bool,
    cancelled: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    result: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct PreparedRunner {
    command: Command,
    last_message_path: Option<PathBuf>,
    _temp_home: Option<TempDir>,
}

#[derive(Debug, Clone, Copy)]
enum RunnerKind {
    Claude,
    Codex,
}

pub fn start(db: Arc<Database>) -> Result<DispatchDiscovery, AppError> {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AppError::Message(format!("启动 Dispatch 服务失败: {e}")))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|e| AppError::Message(format!("设置 Dispatch 服务非阻塞失败: {e}")))?;
    let address = std_listener
        .local_addr()
        .map_err(|e| AppError::Message(format!("获取 Dispatch 服务地址失败: {e}")))?;

    let discovery = DispatchDiscovery {
        base_url: format!("http://{}", address),
        token: Uuid::new_v4().to_string(),
        pid: std::process::id(),
        updated_at: Utc::now().timestamp(),
    };
    write_discovery(&discovery)?;
    reconcile_interrupted_dispatch_runs(&db)?;
    refresh_status_snapshot_from_db(&db);

    let router = Router::new()
        .route("/health", get(health_check))
        .route("/v1/agent/plan", post(plan_agent))
        .route("/v1/agent/run", post(run_agent))
        .route("/v1/agent/runs", get(list_dispatch_runs))
        .route("/v1/agent/runs/:run_id", get(get_dispatch_run))
        .route("/v1/agent/runs/:run_id/cancel", post(cancel_dispatch_run))
        .route("/v1/agent/providers", get(list_dispatch_providers))
        .route("/v1/dispatch/providers", get(list_dispatch_providers))
        .route("/v1/dispatch/run", post(run_dispatch))
        .route("/v1/dispatch/bridge", post(prepare_bridge_dispatch))
        .route("/v1/dispatch/runs", get(list_dispatch_runs))
        .route("/v1/dispatch/runs/:run_id", get(get_dispatch_run))
        .route(
            "/v1/dispatch/runs/:run_id/bridge-start",
            post(mark_bridge_dispatch_run_started),
        )
        .route(
            "/v1/dispatch/runs/:run_id/bridge-complete",
            post(complete_bridge_dispatch_run),
        )
        .route(
            "/v1/dispatch/runs/:run_id/cancel",
            post(cancel_dispatch_run),
        )
        .with_state(DispatchApiState {
            db,
            token: Arc::new(discovery.token.clone()),
            base_url: Arc::new(discovery.base_url.clone()),
            cancel_senders: Arc::new(Mutex::new(HashMap::new())),
        });

    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::from_std(std_listener) {
            Ok(listener) => listener,
            Err(err) => {
                log::error!("Failed to initialize dispatch listener inside runtime: {err}");
                return;
            }
        };

        if let Err(err) = axum::serve(listener, router).await {
            log::error!("Dispatch service stopped unexpectedly: {err}");
        }
    });

    log::info!("Dispatch service listening on {}", discovery.base_url);
    Ok(discovery)
}

fn write_discovery(discovery: &DispatchDiscovery) -> Result<(), AppError> {
    let path = get_app_config_dir().join(DISCOVERY_FILE_NAME);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let payload = serde_json::to_vec_pretty(discovery)
        .map_err(|e| AppError::Message(format!("序列化 Dispatch 服务发现文件失败: {e}")))?;
    fs::write(&path, payload).map_err(|e| AppError::io(&path, e))?;
    Ok(())
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn list_dispatch_providers(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<DispatchProvidersResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let app_filter = query.get("app").map(String::as_str);
    let app_filter = match app_filter {
        Some(raw) if !raw.is_empty() => Some(parse_dispatchable_app(raw)?),
        _ => None,
    };

    let providers = collect_dispatch_providers(&state.db, app_filter.as_ref())
        .map_err(internal_error)?;
    Ok(Json(DispatchProvidersResponse { providers }))
}

async fn plan_agent(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    Json(request): Json<AgentPlanRequest>,
) -> Result<Json<AgentPlanResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let task = request.task.trim().to_string();
    if task.is_empty() {
        return Err(bad_request("Task content cannot be empty"));
    }

    let cwd = normalize_cwd(request.cwd.as_deref())?;
    let providers = collect_dispatch_providers(&state.db, None).map_err(internal_error)?;
    let plan = build_agent_plan(
        &providers,
        &task,
        request.policy.as_deref(),
        request.target.as_deref(),
        request.mode.as_deref(),
    )
    .map_err(internal_error)?;

    Ok(Json(AgentPlanResponse {
        accepted: true,
        task,
        cwd: cwd.display().to_string(),
        plan,
    }))
}

async fn run_agent(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    Json(request): Json<AgentRunRequest>,
) -> Result<Json<AgentRunResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let task = request.task.trim().to_string();
    if task.is_empty() {
        return Err(bad_request("Task content cannot be empty"));
    }

    let cwd = normalize_cwd(request.cwd.as_deref())?;
    let cwd_display = cwd.display().to_string();
    let timeout_seconds = normalize_timeout(request.timeout_seconds);
    let wait_for_completion = request.wait_for_completion.unwrap_or(false);
    let providers = collect_dispatch_providers(&state.db, None).map_err(internal_error)?;
    let plan = build_agent_plan(
        &providers,
        &task,
        request.policy.as_deref(),
        request.target.as_deref(),
        request.mode.as_deref(),
    )
    .map_err(internal_error)?;

    let dispatch_request = DispatchRunRequest {
        target: plan.recommended_target.clone(),
        task,
        timeout_seconds: Some(timeout_seconds),
        cwd: Some(cwd_display),
        wait_for_completion: Some(wait_for_completion),
        route_policy: Some(plan.route_policy.clone()),
        task_kind: Some(plan.task_kind.clone()),
        reasoning_level: Some(plan.reasoning_level.clone()),
        runtime_mode: Some(plan.preferred_runtime.clone()),
        callback_mode: Some(
            if plan.preferred_runtime == AGENT_RUNTIME_PANE {
                "auto"
            } else {
                "manual"
            }
            .to_string(),
        ),
    };

    let Json(run) = run_dispatch(State(state), headers, Json(dispatch_request)).await?;
    let legacy_command_hint = format!(
        "/dispatch-task {} timeout={}{} -- <task text>",
        plan.recommended_target,
        timeout_seconds,
        if plan.preferred_runtime == AGENT_RUNTIME_PANE {
            " monitor=pane"
        } else if wait_for_completion {
            " wait=true"
        } else {
            ""
        }
    );

    Ok(Json(AgentRunResponse {
        accepted: true,
        completed: run.completed,
        run,
        plan,
        legacy_command_hint,
    }))
}

async fn run_dispatch(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    Json(request): Json<DispatchRunRequest>,
) -> Result<Json<DispatchRunResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let parsed_target = parse_target(&request.target)?;
    let cwd = normalize_cwd(request.cwd.as_deref())?;
    let timeout_seconds = normalize_timeout(request.timeout_seconds);
    let wait_for_completion = request.wait_for_completion.unwrap_or(false);
    let task = request.task.trim().to_string();
    let route_policy = normalize_route_policy(request.route_policy.as_deref());
    let task_kind = normalize_task_kind(request.task_kind.as_deref());
    let reasoning_level = normalize_reasoning_level(request.reasoning_level.as_deref());
    let runtime_mode = normalize_runtime_mode(request.runtime_mode.as_deref(), wait_for_completion)?;
    let callback_mode = normalize_callback_mode_for_run(request.callback_mode.as_deref())?;

    if task.is_empty() {
        return Err(bad_request("Task content cannot be empty"));
    }

    let provider = load_effective_provider(&state.db, &parsed_target).map_err(internal_error)?;
    let provider_name = provider.name.clone();

    let runner = match parsed_target.app {
        AppType::Claude => RunnerKind::Claude,
        AppType::Codex => RunnerKind::Codex,
        _ => return Err(bad_request("Only Claude and Codex targets are supported")),
    };
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now().timestamp();
    let cwd_display = cwd.display().to_string();
    let host_app = "claude".to_string();
    let task_preview = truncate_preview(&task);
    insert_dispatch_run(
        &state.db,
        DispatchRunRecord {
            run_id: run_id.clone(),
            target: request.target.clone(),
            provider_name: provider_name.clone(),
            host_app,
            route_policy,
            task_kind,
            reasoning_level,
            runtime_mode,
            callback_mode,
            cwd: cwd_display.clone(),
            task_preview,
            status: "queued".to_string(),
            timeout_seconds,
            started_at,
            updated_at: started_at,
            finished_at: None,
            exit_code: None,
            duration_ms: None,
            timed_out: false,
            cancelled: false,
            result_preview: String::new(),
            result: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        },
    )
    .map_err(internal_error)?;

    refresh_status_snapshot_from_db(&state.db);
    let (tx, rx) = oneshot::channel();
    let cancel_senders = state.cancel_senders.clone();
    let db = state.db.clone();
    let target = request.target.clone();
    let provider_name_for_task = provider.name.clone();
    let run_id_for_task = run_id.clone();
    let cwd_for_task = cwd.clone();

    tauri::async_runtime::spawn(async move {
        let response = execute_dispatch_run(
            db,
            cancel_senders,
            run_id_for_task.clone(),
            started_at,
            target,
            provider_name_for_task,
            cwd_for_task,
            timeout_seconds,
            task,
            provider,
            runner,
        )
        .await;
        let _ = tx.send(response);
    });

    if wait_for_completion {
        let response = rx.await.map_err(|_| {
            internal_error(AppError::Message(
                "Dispatch run stopped before returning a result".to_string(),
            ))
        })?;
        return Ok(Json(response));
    }

    Ok(Json(DispatchRunResponse::accepted(
        run_id,
        request.target,
        provider_name,
        started_at,
        timeout_seconds,
        cwd_display,
    )))
}

async fn prepare_bridge_dispatch(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    Json(request): Json<DispatchBridgePrepareRequest>,
) -> Result<Json<DispatchBridgePrepareResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let parsed_target = parse_target(&request.target)?;
    if parsed_target.app != AppType::Codex {
        return Err(bad_request("tmux bridge currently requires a codex target"));
    }

    let callback_pane = request.callback_pane.trim();
    if callback_pane.is_empty() {
        return Err(bad_request("callbackPane cannot be empty"));
    }

    let callback_mode = normalize_callback_mode(request.callback_mode.as_deref())?;
    let cwd = normalize_cwd(request.cwd.as_deref())?;
    let timeout_seconds = normalize_timeout(request.timeout_seconds);
    let task = request.task.trim().to_string();
    let route_policy = normalize_route_policy(request.route_policy.as_deref());
    let task_kind = normalize_task_kind(request.task_kind.as_deref());
    let reasoning_level = normalize_reasoning_level(request.reasoning_level.as_deref());
    if task.is_empty() {
        return Err(bad_request("Task content cannot be empty"));
    }

    let provider = load_effective_provider(&state.db, &parsed_target).map_err(internal_error)?;
    let provider_name = provider.name.clone();
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now().timestamp();
    let cwd_display = cwd.display().to_string();
    let host_app = request
        .host_app
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("claude")
        .to_string();

    insert_dispatch_run(
        &state.db,
        DispatchRunRecord {
            run_id: run_id.clone(),
            target: request.target.clone(),
            provider_name: provider_name.clone(),
            host_app,
            route_policy,
            task_kind,
            reasoning_level,
            runtime_mode: AGENT_RUNTIME_PANE.to_string(),
            callback_mode: callback_mode.to_string(),
            cwd: cwd_display.clone(),
            task_preview: truncate_preview(&task),
            status: "queued".to_string(),
            timeout_seconds,
            started_at,
            updated_at: started_at,
            finished_at: None,
            exit_code: None,
            duration_ms: None,
            timed_out: false,
            cancelled: false,
            result_preview: String::new(),
            result: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        },
    )
    .map_err(internal_error)?;

    let spec_path = prepare_codex_bridge_spec(
        &provider,
        &cwd,
        &task,
        &run_id,
        &request.target,
        state.base_url.as_str(),
        state.token.as_str(),
        callback_pane,
        callback_mode,
        timeout_seconds,
    )
    .map_err(internal_error)?;

    refresh_status_snapshot_from_db(&state.db);
    Ok(Json(DispatchBridgePrepareResponse {
        accepted: true,
        completed: false,
        run_id,
        target: request.target,
        provider_name,
        state: "queued".to_string(),
        started_at,
        timeout_seconds,
        cwd: cwd_display,
        spec_path: spec_path.display().to_string(),
        callback_pane: callback_pane.to_string(),
        callback_mode: callback_mode.to_string(),
    }))
}

async fn list_dispatch_runs(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    Query(query): Query<DispatchRunsQuery>,
) -> Result<Json<DispatchRunsResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let limit = query.limit.unwrap_or(10).clamp(1, 100);
    let runs = load_dispatch_runs(&state.db, limit, query.status.as_deref()).map_err(internal_error)?;
    Ok(Json(DispatchRunsResponse { runs }))
}

async fn get_dispatch_run(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<DispatchRunRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;
    let run = load_dispatch_run(&state.db, &run_id)
        .map_err(internal_error)?
        .ok_or_else(|| bad_request(format!("Dispatch run '{run_id}' was not found")))?;
    Ok(Json(run))
}

async fn cancel_dispatch_run(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<DispatchCancelResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let sender = {
        let guard = state.cancel_senders.lock().await;
        guard.get(&run_id).cloned()
    };

    let Some(sender) = sender else {
        let run = load_dispatch_run(&state.db, &run_id)
            .map_err(internal_error)?
            .ok_or_else(|| bad_request(format!("Dispatch run '{run_id}' was not found")))?;
        return Ok(Json(DispatchCancelResponse { ok: false, run }));
    };

    let _ = sender.send(true);
    let run = load_dispatch_run(&state.db, &run_id)
        .map_err(internal_error)?
        .ok_or_else(|| bad_request(format!("Dispatch run '{run_id}' was not found")))?;
    Ok(Json(DispatchCancelResponse { ok: true, run }))
}

async fn mark_bridge_dispatch_run_started(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
    Json(request): Json<DispatchBridgeStartedRequest>,
) -> Result<Json<DispatchRunRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;
    if let Some(pane_id) = request.pane_id.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        log::info!("Bridge dispatch run {run_id} started in tmux pane {pane_id}");
    }
    mark_dispatch_run_running(&state.db, &run_id).map_err(internal_error)?;
    let run = load_dispatch_run(&state.db, &run_id)
        .map_err(internal_error)?
        .ok_or_else(|| bad_request(format!("Dispatch run '{run_id}' was not found")))?;
    Ok(Json(run))
}

async fn complete_bridge_dispatch_run(
    State(state): State<DispatchApiState>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
    Json(request): Json<DispatchBridgeCompleteRequest>,
) -> Result<Json<DispatchRunResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    authorize(&headers, state.token.as_str())?;

    let run = load_dispatch_run(&state.db, &run_id)
        .map_err(internal_error)?
        .ok_or_else(|| bad_request(format!("Dispatch run '{run_id}' was not found")))?;

    let status = normalize_bridge_completion_status(
        request.status.trim(),
        request.timed_out,
        request.cancelled,
    )?;
    let history_entry = DispatchHistoryEntry {
        run_id: run.run_id.clone(),
        timestamp: Utc::now().timestamp(),
        target: run.target.clone(),
        provider_name: run.provider_name.clone(),
        cwd: run.cwd.clone(),
        timeout_seconds: run.timeout_seconds,
        status: status.to_string(),
        timed_out: request.timed_out,
        cancelled: request.cancelled,
        exit_code: request.exit_code,
        duration_ms: request.duration_ms,
        result_preview: truncate_preview(&request.result),
        result: request.result,
        stdout: request.stdout,
        stderr: request.stderr,
    };

    finalize_dispatch_run(&state.db, &history_entry).map_err(internal_error)?;
    append_history(history_entry.clone());
    refresh_status_snapshot_from_db(&state.db);

    Ok(Json(DispatchRunResponse::completed(
        run.run_id,
        run.target,
        run.provider_name,
        run.started_at,
        run.timeout_seconds,
        run.cwd,
        &history_entry,
    )))
}

impl RunnerKind {
    fn prepare(
        self,
        provider: &Provider,
        cwd: &Path,
        task: &str,
    ) -> Result<PreparedRunner, AppError> {
        match self {
            RunnerKind::Claude => prepare_claude_command(provider, cwd, task),
            RunnerKind::Codex => prepare_codex_command(provider, cwd, task),
        }
    }
}

fn prepare_claude_command(
    provider: &Provider,
    cwd: &Path,
    task: &str,
) -> Result<PreparedRunner, AppError> {
    let envs = extract_claude_env(&provider.settings_config)?;
    let wrapped_task = wrap_task_for_main_agent(task);
    let permission_mode = claude_dispatch_permission_mode(&provider.settings_config);
    let claude_executable = resolve_cli_executable("claude")
        .ok_or_else(|| missing_cli_error("claude"))?;

    let mut command = Command::new(&claude_executable);
    command
        .current_dir(cwd)
        .arg("-p")
        .arg("--permission-mode")
        .arg(permission_mode)
        .arg("--disable-slash-commands")
        .arg("--no-session-persistence")
        .arg("--add-dir")
        .arg(cwd)
        .arg("--")
        .arg(&wrapped_task)
        .kill_on_drop(true);
    prepend_command_dir_to_path(&mut command, &claude_executable);

    for key in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_REASONING_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ] {
        command.env_remove(key);
    }
    command.envs(envs.iter().map(|(k, v)| (k, v)));

    Ok(PreparedRunner {
        command,
        last_message_path: None,
        _temp_home: None,
    })
}

fn claude_dispatch_permission_mode(settings: &Value) -> &'static str {
    match settings
        .get("dispatchPermissionMode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some("acceptEdits") => "acceptEdits",
        Some("default") => "default",
        Some("dontAsk") => "dontAsk",
        Some("plan") => "plan",
        Some("auto") => "auto",
        Some("bypassPermissions") => "bypassPermissions",
        _ => "bypassPermissions",
    }
}

fn prepare_codex_command(
    provider: &Provider,
    cwd: &Path,
    task: &str,
) -> Result<PreparedRunner, AppError> {
    let wrapped_task = wrap_task_for_main_agent(task);
    let temp_home = TempDir::new()
        .map_err(|e| AppError::Message(format!("创建 Codex 临时目录失败: {e}")))?;
    let codex_dir = temp_home.path().join(".codex");
    fs::create_dir_all(&codex_dir).map_err(|e| AppError::io(&codex_dir, e))?;

    let auth_path = codex_dir.join("auth.json");
    let config_path = codex_dir.join("config.toml");
    let last_message_path = temp_home.path().join("last-message.txt");

    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::Message("Codex provider configuration must be an object".to_string())
    })?;
    let auth = settings.get("auth").ok_or_else(|| {
        AppError::Message(format!("Codex provider '{}' is missing auth", provider.id))
    })?;
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::Message(format!("Codex provider '{}' is missing config.toml", provider.id))
        })?;

    let auth_payload = serde_json::to_vec_pretty(auth)
        .map_err(|e| AppError::Message(format!("序列化 Codex auth.json 失败: {e}")))?;
    fs::write(&auth_path, auth_payload).map_err(|e| AppError::io(&auth_path, e))?;
    fs::write(&config_path, config).map_err(|e| AppError::io(&config_path, e))?;
    let codex_executable = resolve_cli_executable("codex")
        .ok_or_else(|| missing_cli_error("codex"))?;

    let mut command = Command::new(&codex_executable);
    command
        .current_dir(cwd)
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("-s")
        .arg("read-only")
        .arg("-C")
        .arg(cwd)
        .arg("-o")
        .arg(&last_message_path)
        .arg(&wrapped_task)
        .kill_on_drop(true)
        .env("HOME", temp_home.path())
        .env("CODEX_HOME", &codex_dir)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL");
    prepend_command_dir_to_path(&mut command, &codex_executable);

    Ok(PreparedRunner {
        command,
        last_message_path: Some(last_message_path),
        _temp_home: Some(temp_home),
    })
}

fn prepare_codex_bridge_spec(
    provider: &Provider,
    cwd: &Path,
    task: &str,
    run_id: &str,
    target: &str,
    base_url: &str,
    token: &str,
    callback_pane: &str,
    callback_mode: &str,
    timeout_seconds: u64,
) -> Result<PathBuf, AppError> {
    let wrapped_task = wrap_task_for_main_agent(task);
    let bridge_dir = get_app_config_dir().join("dispatch-bridge").join(run_id);
    let codex_dir = bridge_dir.join(".codex");
    fs::create_dir_all(&codex_dir).map_err(|e| AppError::io(&codex_dir, e))?;

    let auth_path = codex_dir.join("auth.json");
    let config_path = codex_dir.join("config.toml");
    let last_message_path = bridge_dir.join("last-message.txt");
    let spec_path = bridge_dir.join("bridge-spec.json");

    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::Message("Codex provider configuration must be an object".to_string())
    })?;
    let auth = settings.get("auth").ok_or_else(|| {
        AppError::Message(format!("Codex provider '{}' is missing auth", provider.id))
    })?;
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::Message(format!("Codex provider '{}' is missing config.toml", provider.id))
        })?;

    let auth_payload = serde_json::to_vec_pretty(auth)
        .map_err(|e| AppError::Message(format!("序列化 Codex auth.json 失败: {e}")))?;
    fs::write(&auth_path, auth_payload).map_err(|e| AppError::io(&auth_path, e))?;
    fs::write(&config_path, config).map_err(|e| AppError::io(&config_path, e))?;

    let codex_executable = resolve_cli_executable("codex")
        .ok_or_else(|| missing_cli_error("codex"))?;
    let mut env = HashMap::new();
    env.insert("HOME".to_string(), bridge_dir.display().to_string());
    env.insert("CODEX_HOME".to_string(), codex_dir.display().to_string());

    let spec = DispatchBridgeLaunchSpec {
        base_url: base_url.to_string(),
        token: token.to_string(),
        run_id: run_id.to_string(),
        target: target.to_string(),
        provider_name: provider.name.clone(),
        cwd: cwd.display().to_string(),
        timeout_seconds,
        callback_pane: callback_pane.to_string(),
        callback_mode: callback_mode.to_string(),
        command: vec![
            codex_executable.display().to_string(),
            "exec".to_string(),
            "--skip-git-repo-check".to_string(),
            "-s".to_string(),
            "read-only".to_string(),
            "-C".to_string(),
            cwd.display().to_string(),
            "-o".to_string(),
            last_message_path.display().to_string(),
            wrapped_task,
        ],
        env,
        env_remove: vec!["OPENAI_API_KEY".to_string(), "OPENAI_BASE_URL".to_string()],
        path_prefix: codex_executable.parent().map(|path| path.display().to_string()),
        last_message_path: last_message_path.display().to_string(),
    };

    let payload = serde_json::to_vec_pretty(&spec)
        .map_err(|e| AppError::Message(format!("序列化 bridge spec 失败: {e}")))?;
    fs::write(&spec_path, payload).map_err(|e| AppError::io(&spec_path, e))?;
    Ok(spec_path)
}

async fn run_subprocess_streaming(
    db: Arc<Database>,
    cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    run_id: &str,
    mut prepared: PreparedRunner,
    timeout_seconds: u64,
) -> Result<RunnerOutput, AppError> {
    let started = std::time::Instant::now();
    prepared.command.stdin(Stdio::null());
    prepared.command.stdout(Stdio::piped());
    prepared.command.stderr(Stdio::piped());
    let mut child = prepared.command.spawn().map_err(map_spawn_error)?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    {
        let mut guard = cancel_senders.lock().await;
        guard.insert(run_id.to_string(), cancel_tx);
    }

    mark_dispatch_run_running(&db, run_id)?;

    let stdout_task = stdout.map(|stream| {
        let db = db.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move { stream_output_to_store(db, run_id, stream, true).await })
    });
    let stderr_task = stderr.map(|stream| {
        let db = db.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move { stream_output_to_store(db, run_id, stream, false).await })
    });

    let wait_result = tokio::select! {
        result = child.wait() => WaitOutcome::Exited(result.map_err(|err| AppError::Message(format!("等待子进程结果失败: {err}")))?),
        _ = tokio::time::sleep(Duration::from_secs(timeout_seconds)) => {
            let _ = child.kill().await;
            WaitOutcome::TimedOut
        }
        changed = cancel_rx.changed() => {
            match changed {
                Ok(_) if *cancel_rx.borrow() => {
                    let _ = child.kill().await;
                    WaitOutcome::Cancelled
                }
                Ok(_) => WaitOutcome::Cancelled,
                Err(_) => WaitOutcome::Cancelled,
            }
        }
    };

    if let Some(task) = stdout_task {
        let _ = task.await;
    }
    if let Some(task) = stderr_task {
        let _ = task.await;
    }

    {
        let mut guard = cancel_senders.lock().await;
        guard.remove(run_id);
    }

    let record = load_dispatch_run(&db, run_id)?.ok_or_else(|| {
        AppError::Message(format!("Dispatch run '{run_id}' disappeared while executing"))
    })?;
    let stdout = record.stdout.trim().to_string();
    let stderr = record.stderr.trim().to_string();
    let duration_ms = started.elapsed().as_millis() as u64;
    let mut result = prepared
        .last_message_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| stdout.clone());

    let output = match wait_result {
        WaitOutcome::Exited(status) => {
            if result.is_empty() {
                result = stderr.clone();
            }
            if result.is_empty() {
                result = "Command completed without output.".to_string();
            }
            RunnerOutput {
                status: if status.success() { "succeeded" } else { "failed" },
                timed_out: false,
                cancelled: false,
                exit_code: status.code(),
                duration_ms,
                result,
                stdout,
                stderr,
            }
        }
        WaitOutcome::TimedOut => RunnerOutput {
            status: "timed_out",
            timed_out: true,
            cancelled: false,
            exit_code: None,
            duration_ms,
            result: format!("Dispatch request timed out after {timeout_seconds} seconds."),
            stdout,
            stderr,
        },
        WaitOutcome::Cancelled => RunnerOutput {
            status: "cancelled",
            timed_out: false,
            cancelled: true,
            exit_code: None,
            duration_ms,
            result: "Dispatch run was cancelled.".to_string(),
            stdout,
            stderr,
        },
    };

    Ok(output)
}

async fn stream_output_to_store<R>(
    db: Arc<Database>,
    run_id: String,
    mut reader: R,
    stdout: bool,
) -> Result<(), AppError>
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0u8; 4096];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|err| AppError::Message(format!("读取子进程输出失败: {err}")))?;
        if read == 0 {
            break;
        }
        let chunk = String::from_utf8_lossy(&buffer[..read]).to_string();
        append_dispatch_output_chunk(&db, &run_id, &chunk, stdout)?;
    }
    Ok(())
}

async fn execute_dispatch_run(
    db: Arc<Database>,
    cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    run_id: String,
    started_at: i64,
    target: String,
    provider_name: String,
    cwd: PathBuf,
    timeout_seconds: u64,
    task: String,
    provider: Provider,
    runner: RunnerKind,
) -> DispatchRunResponse {
    let cwd_display = cwd.display().to_string();
    let output = match runner.prepare(&provider, &cwd, &task) {
        Ok(prepared) => match run_subprocess_streaming(
            db.clone(),
            cancel_senders,
            &run_id,
            prepared,
            timeout_seconds,
        )
        .await
        {
            Ok(output) => output,
            Err(err) => failure_output_from_error(err),
        },
        Err(err) => failure_output_from_error(err),
    };

    let history_entry = history_entry_from_output(
        run_id.clone(),
        target.clone(),
        provider_name.clone(),
        cwd_display.clone(),
        timeout_seconds,
        output,
    );
    if let Err(err) = finalize_dispatch_run(&db, &history_entry) {
        log::warn!("Failed to persist completed dispatch run {}: {err}", run_id);
    }
    append_history(history_entry.clone());
    refresh_status_snapshot_from_db(&db);

    DispatchRunResponse::completed(
        run_id,
        target,
        provider_name,
        started_at,
        timeout_seconds,
        cwd_display,
        &history_entry,
    )
}

#[derive(Debug)]
enum WaitOutcome {
    Exited(std::process::ExitStatus),
    TimedOut,
    Cancelled,
}

fn failure_output_from_error(err: AppError) -> RunnerOutput {
    let error_message = err.to_string();
    RunnerOutput {
        status: "failed",
        timed_out: false,
        cancelled: false,
        exit_code: None,
        duration_ms: 0,
        result: error_message.clone(),
        stdout: String::new(),
        stderr: error_message,
    }
}

fn history_entry_from_output(
    run_id: String,
    target: String,
    provider_name: String,
    cwd: String,
    timeout_seconds: u64,
    output: RunnerOutput,
) -> DispatchHistoryEntry {
    DispatchHistoryEntry {
        run_id,
        timestamp: Utc::now().timestamp(),
        target,
        provider_name,
        cwd,
        timeout_seconds,
        status: output.status.to_string(),
        timed_out: output.timed_out,
        cancelled: output.cancelled,
        exit_code: output.exit_code,
        duration_ms: output.duration_ms,
        result_preview: truncate_preview(&output.result),
        result: output.result,
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

impl DispatchRunResponse {
    fn accepted(
        run_id: String,
        target: String,
        provider_name: String,
        started_at: i64,
        timeout_seconds: u64,
        cwd: String,
    ) -> Self {
        Self {
            accepted: true,
            completed: false,
            run_id,
            target,
            provider_name,
            state: "queued".to_string(),
            started_at,
            timeout_seconds,
            cwd,
            ok: None,
            status: None,
            timed_out: None,
            cancelled: None,
            exit_code: None,
            duration_ms: None,
            result: None,
            stdout: None,
            stderr: None,
        }
    }

    fn completed(
        run_id: String,
        target: String,
        provider_name: String,
        started_at: i64,
        timeout_seconds: u64,
        cwd: String,
        history_entry: &DispatchHistoryEntry,
    ) -> Self {
        let status = history_entry.status.as_str();
        let result_status = if status == "succeeded" {
            "succeeded"
        } else if status == "timed_out" {
            "timed_out"
        } else if status == "cancelled" {
            "cancelled"
        } else {
            "failed"
        };

        Self {
            accepted: true,
            completed: true,
            run_id,
            target,
            provider_name,
            state: "finished".to_string(),
            started_at,
            timeout_seconds,
            cwd,
            ok: Some(result_status == "succeeded"),
            status: Some(result_status.to_string()),
            timed_out: Some(history_entry.timed_out),
            cancelled: Some(history_entry.cancelled),
            exit_code: history_entry.exit_code,
            duration_ms: Some(history_entry.duration_ms),
            result: Some(history_entry.result.clone()),
            stdout: Some(history_entry.stdout.clone()),
            stderr: Some(history_entry.stderr.clone()),
        }
    }
}

fn resolve_cli_executable(tool: &str) -> Option<PathBuf> {
    for dir in cli_search_paths() {
        for candidate in cli_executable_candidates(tool, &dir) {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn cli_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(path_value) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&path_value) {
            push_unique_search_path(&mut paths, path);
        }
    }

    let home = dirs::home_dir().unwrap_or_default();
    if !home.as_os_str().is_empty() {
        for path in [
            home.join(".deskclaw").join("node").join("bin"),
            home.join(".local").join("bin"),
            home.join(".npm-global").join("bin"),
            home.join(".n").join("bin"),
            home.join(".volta").join("bin"),
            home.join(".yarn").join("bin"),
            home.join(".bun").join("bin"),
            home.join("bin"),
        ] {
            push_unique_search_path(&mut paths, path);
        }

        let fnm_base = home.join(".local").join("state").join("fnm_multishells");
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                push_unique_search_path(&mut paths, entry.path().join("bin"));
            }
        }

        let nvm_base = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                push_unique_search_path(&mut paths, entry.path().join("bin"));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        push_unique_search_path(&mut paths, PathBuf::from("/opt/homebrew/bin"));
        push_unique_search_path(&mut paths, PathBuf::from("/usr/local/bin"));
        push_unique_search_path(&mut paths, PathBuf::from("/usr/bin"));
    }

    #[cfg(target_os = "linux")]
    {
        push_unique_search_path(&mut paths, PathBuf::from("/usr/local/bin"));
        push_unique_search_path(&mut paths, PathBuf::from("/usr/bin"));
    }

    paths
}

fn cli_executable_candidates(tool: &str, dir: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        vec![
            dir.join(format!("{tool}.cmd")),
            dir.join(format!("{tool}.exe")),
            dir.join(tool),
        ]
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![dir.join(tool)]
    }
}

fn push_unique_search_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() {
        return;
    }
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn prepend_command_dir_to_path(command: &mut Command, executable: &Path) {
    let Some(dir) = executable.parent() else {
        return;
    };

    let mut paths = vec![dir.to_path_buf()];
    if let Some(current_path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current_path));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        command.env("PATH", joined);
    }
}

fn missing_cli_error(tool: &str) -> AppError {
    AppError::Message(format!(
        "Required CLI tool '{tool}' is not installed or not found in PATH/common install directories"
    ))
}

fn extract_claude_env(settings: &Value) -> Result<HashMap<String, String>, AppError> {
    let env = settings
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Message("Claude provider configuration is missing env".to_string()))?;

    let mut envs = HashMap::new();
    for (key, value) in env {
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                envs.insert(key.clone(), trimmed.to_string());
            }
        }
    }

    let has_auth = envs.contains_key("ANTHROPIC_AUTH_TOKEN") || envs.contains_key("ANTHROPIC_API_KEY");
    if !has_auth {
        return Err(AppError::Message(
            "Claude provider is missing ANTHROPIC_AUTH_TOKEN or ANTHROPIC_API_KEY".to_string(),
        ));
    }

    Ok(envs)
}

fn collect_dispatch_providers(
    db: &Arc<Database>,
    app_filter: Option<&AppType>,
) -> Result<Vec<DispatchProviderTarget>, AppError> {
    let mut providers = Vec::new();

    for app in [AppType::Claude, AppType::Codex] {
        if let Some(filter) = app_filter {
            if filter != &app {
                continue;
            }
        }

        let current_id = crate::settings::get_effective_current_provider(db, &app)?.unwrap_or_default();
        let all = db.get_all_providers(app.as_str())?;
        let mut selector_counts = HashMap::new();
        for provider in all.values() {
            let selector = preferred_dispatch_selector(provider);
            *selector_counts.entry(selector).or_insert(0usize) += 1;
        }
        for provider in all.values() {
            let effective_settings = build_effective_settings_with_common_config(db, &app, provider)?;
            if !provider_is_dispatchable(&app, &effective_settings) {
                continue;
            }
            let preferred_selector = preferred_dispatch_selector(provider);
            let preferred_is_unique = selector_counts.get(&preferred_selector).copied().unwrap_or_default() == 1;
            let target_selector = if preferred_is_unique {
                preferred_selector
            } else {
                provider.id.clone()
            };
            providers.push(DispatchProviderTarget {
                target: format!("{}:{}", app.as_str(), target_selector),
                canonical_target: format!("{}:{}", app.as_str(), provider.id),
                app: app.as_str().to_string(),
                provider_id: provider.id.clone(),
                provider_name: provider.name.clone(),
                current: provider.id == current_id,
            });
        }
    }

    providers.sort_by(|left, right| {
        left.app
            .cmp(&right.app)
            .then_with(|| left.provider_name.to_lowercase().cmp(&right.provider_name.to_lowercase()))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });

    Ok(providers)
}

fn preferred_dispatch_selector(provider: &Provider) -> String {
    let preferred = provider
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
        .unwrap_or_else(|| dispatch_name_slug(&provider.name));

    if preferred.is_empty() {
        provider.id.clone()
    } else {
        preferred
    }
}

fn dispatch_name_slug(name: &str) -> String {
    let sanitized = sanitize_provider_name(name);
    let mut out = String::with_capacity(sanitized.len());
    let mut last_dash = false;
    for ch in sanitized.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn provider_is_dispatchable(app: &AppType, settings: &Value) -> bool {
    match app {
        AppType::Claude => extract_claude_env(settings).is_ok(),
        AppType::Codex => settings.get("auth").and_then(Value::as_object).is_some()
            && settings
                .get("config")
                .and_then(Value::as_str)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
        _ => false,
    }
}

fn wrap_task_for_main_agent(task: &str) -> String {
    format!(
        r#"You are a dispatched sub-agent working for a main agent.

Complete the assigned subtask below. When you finish, you MUST explicitly callback to the main agent and tell it that you have finished.

Requirements for your final response:
1. Do the requested work first.
2. End with a callback block using this exact structure.
3. If you completed the task, use `status: completed` and `message: 我已经实现完了`.
4. If you are blocked or could not finish, use `status: blocked` and explain the blocker.

Required callback block:
<<{tag}>>
status: completed|blocked
message: 我已经实现完了
summary: <one-sentence summary for the main agent>
deliverable:
<the concrete result or handoff for the main agent>
<</{tag}>>

Assigned subtask:
{task}"#,
        tag = MAIN_AGENT_CALLBACK_TAG,
        task = task.trim()
    )
}

fn load_effective_provider(db: &Arc<Database>, target: &ParsedTarget) -> Result<Provider, AppError> {
    let provider = resolve_dispatch_provider(db, target)?.ok_or_else(|| {
        AppError::Message(format!(
            "Dispatch target '{}' does not exist in cc-switch",
            format!("{}:{}", target.app.as_str(), target.provider_selector)
        ))
    })?;

    let mut effective = provider.clone();
    effective.settings_config = build_effective_settings_with_common_config(db, &target.app, &provider)?;
    Ok(effective)
}

fn resolve_dispatch_provider(
    db: &Arc<Database>,
    target: &ParsedTarget,
) -> Result<Option<Provider>, AppError> {
    let selector = target.provider_selector.trim();
    if selector.eq_ignore_ascii_case("current") {
        let current_id =
            crate::settings::get_effective_current_provider(db, &target.app)?.unwrap_or_default();
        if current_id.is_empty() {
            return Ok(None);
        }
        return db.get_provider_by_id(&current_id, target.app.as_str());
    }

    if let Some(provider) = db.get_provider_by_id(selector, target.app.as_str())? {
        return Ok(Some(provider));
    }

    let selector_lower = selector.to_lowercase();
    let providers = db.get_all_providers(target.app.as_str())?;

    if let Some(provider) = providers
        .values()
        .find(|provider| {
            provider
                .alias
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_lowercase)
                .as_deref()
                == Some(selector_lower.as_str())
        })
        .cloned()
    {
        return Ok(Some(provider));
    }

    if let Some(provider) = providers
        .values()
        .find(|provider| dispatch_name_slug(&provider.name) == selector_lower)
        .cloned()
    {
        return Ok(Some(provider));
    }

    if let Some(provider) = providers
        .values()
        .find(|provider| provider.name.eq_ignore_ascii_case(selector))
        .cloned()
    {
        return Ok(Some(provider));
    }

    Ok(None)
}

fn parse_dispatchable_app(raw: &str) -> Result<AppType, (StatusCode, Json<ApiErrorResponse>)> {
    match raw {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        _ => Err(bad_request("Only 'claude' and 'codex' providers are supported")),
    }
}

fn parse_target(target: &str) -> Result<ParsedTarget, (StatusCode, Json<ApiErrorResponse>)> {
    let Some((app, provider_selector)) = target.split_once(':') else {
        return Err(bad_request(
            "Target must use the form 'claude:<provider>' or 'codex:<provider>'",
        ));
    };

    let app = match app {
        "claude" => AppType::Claude,
        "codex" => AppType::Codex,
        _ => {
            return Err(bad_request(
                "Only 'claude' and 'codex' targets are supported in dispatch v1",
            ));
        }
    };

    let provider_selector = provider_selector.trim();
    if provider_selector.is_empty() {
        return Err(bad_request("Target provider cannot be empty"));
    }

    Ok(ParsedTarget {
        app,
        provider_selector: provider_selector.to_string(),
    })
}

fn build_agent_plan(
    providers: &[DispatchProviderTarget],
    task: &str,
    policy_hint: Option<&str>,
    explicit_target: Option<&str>,
    mode_hint: Option<&str>,
) -> Result<AgentPlan, AppError> {
    let task_kind = infer_task_kind(task, policy_hint);
    let reasoning_level = infer_reasoning_level(&task_kind, task);
    let cost_tier = infer_cost_tier(&task_kind, &reasoning_level);
    let preferred_runtime = normalize_runtime_mode_hint(mode_hint, &task_kind);

    let preferred_app = if let Some(target) = explicit_target {
        parse_target(target)
            .map_err(|_| AppError::Message(format!("Unknown explicit target '{target}'")))?
            .app
    } else if matches!(task_kind.as_str(), "implementation" | "testing") {
        AppType::Codex
    } else {
        AppType::Claude
    };

    let candidates = ranked_agent_targets(
        providers,
        preferred_app,
        explicit_target,
        &task_kind,
        &reasoning_level,
    )?;
    let primary = candidates
        .first()
        .cloned()
        .ok_or_else(|| AppError::Message("No dispatchable providers are available for this task".to_string()))?;

    let explanation = match task_kind.as_str() {
        "architecture" => "This looks architecture-heavy, so the planner keeps Claude-style reasoning in the lead.".to_string(),
        "implementation" => "This looks implementation-heavy, so the plan favors a Codex executor and a terminal-friendly runtime.".to_string(),
        "testing" => "This reads like a testing or bug-fixing task, so the plan favors an execution-oriented child agent.".to_string(),
        "documentation" => "This looks like documentation or summarization work, so the plan keeps the task on a reasoning-first agent.".to_string(),
        _ => "This task stays on a balanced route because no stronger specialization was detected.".to_string(),
    };

    Ok(AgentPlan {
        route_policy: normalize_route_policy(policy_hint),
        task_kind,
        reasoning_level,
        cost_tier,
        preferred_runtime,
        recommended_target: primary.target.clone(),
        recommended_provider_name: primary.provider_name.clone(),
        fallback_chain: candidates.into_iter().skip(1).map(|item| item.target).collect(),
        explanation,
    })
}

fn ranked_agent_targets(
    providers: &[DispatchProviderTarget],
    preferred_app: AppType,
    explicit_target: Option<&str>,
    task_kind: &str,
    reasoning_level: &str,
) -> Result<Vec<DispatchProviderTarget>, AppError> {
    if let Some(target) = explicit_target.map(str::trim).filter(|value| !value.is_empty()) {
        let parsed = parse_target(target)
            .map_err(|_| AppError::Message(format!("Unknown explicit target '{target}'")))?;
        let exact = providers
            .iter()
            .find(|provider| provider.target == target || provider.canonical_target == format!("{}:{}", parsed.app.as_str(), parsed.provider_selector))
            .cloned()
            .ok_or_else(|| AppError::Message(format!("Target '{target}' is not dispatchable")))?;
        let mut ranked = vec![exact.clone()];
        ranked.extend(
            providers
                .iter()
                .filter(|provider| provider.canonical_target != exact.canonical_target)
                .filter(|provider| provider.app == parsed.app.as_str())
                .cloned(),
        );
        return Ok(ranked);
    }

    let mut scored: Vec<(i32, DispatchProviderTarget)> = providers
        .iter()
        .filter(|provider| provider.app == preferred_app.as_str())
        .cloned()
        .map(|provider| {
            let haystack = format!(
                "{} {} {}",
                provider.provider_name.to_lowercase(),
                provider.provider_id.to_lowercase(),
                provider.target.to_lowercase()
            );
            let mut score = if provider.current { 50 } else { 0 };
            if reasoning_level == "high" && (haystack.contains("opus") || haystack.contains("sonnet")) {
                score += 30;
            }
            if reasoning_level == "low" && (haystack.contains("mini") || haystack.contains("haiku") || haystack.contains("flash")) {
                score += 25;
            }
            if task_kind == "implementation" && provider.app == "codex" {
                score += 40;
            }
            if task_kind == "testing" && provider.app == "codex" {
                score += 35;
            }
            if task_kind == "architecture" && provider.app == "claude" {
                score += 40;
            }
            (score, provider)
        })
        .collect();

    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.provider_name.cmp(&right.1.provider_name)));
    let ranked: Vec<_> = scored.into_iter().map(|(_, provider)| provider).collect();
    if ranked.is_empty() {
        return Err(AppError::Message(format!(
            "No dispatchable {} providers are available",
            preferred_app.as_str()
        )));
    }
    Ok(ranked)
}

fn infer_task_kind(task: &str, policy_hint: Option<&str>) -> String {
    let hint = policy_hint.unwrap_or("").trim().to_lowercase();
    if !hint.is_empty() {
        return normalize_task_kind(Some(hint.as_str()));
    }

    let lower = task.to_lowercase();
    if lower.contains("架构")
        || lower.contains("方案")
        || lower.contains("边界")
        || lower.contains("plan")
        || lower.contains("design")
        || lower.contains("architecture")
        || lower.contains("review")
    {
        "architecture".to_string()
    } else if lower.contains("test")
        || lower.contains("测试")
        || lower.contains("fix")
        || lower.contains("bug")
        || lower.contains("回归")
    {
        "testing".to_string()
    } else if lower.contains("doc")
        || lower.contains("readme")
        || lower.contains("文档")
        || lower.contains("总结")
        || lower.contains("summary")
    {
        "documentation".to_string()
    } else if lower.contains("实现")
        || lower.contains("代码")
        || lower.contains("重构")
        || lower.contains("refactor")
        || lower.contains("implement")
        || lower.contains("code")
    {
        "implementation".to_string()
    } else {
        "general".to_string()
    }
}

fn infer_reasoning_level(task_kind: &str, task: &str) -> String {
    let lower = task.to_lowercase();
    if task_kind == "architecture" || lower.contains("复杂") || lower.contains("tradeoff") || lower.contains("风险") {
        "high".to_string()
    } else if task_kind == "implementation" || task_kind == "testing" {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn infer_cost_tier(task_kind: &str, reasoning_level: &str) -> String {
    match (task_kind, reasoning_level) {
        ("architecture", "high") => "premium".to_string(),
        ("implementation", _) | ("testing", _) => "balanced".to_string(),
        _ => "low".to_string(),
    }
}

fn normalize_cwd(raw: Option<&str>) -> Result<PathBuf, (StatusCode, Json<ApiErrorResponse>)> {
    let cwd = match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => std::env::current_dir()
            .map_err(|e| internal_error(AppError::Message(format!("获取当前目录失败: {e}"))))?,
    };

    if !cwd.exists() {
        return Err(bad_request("cwd does not exist"));
    }
    if !cwd.is_dir() {
        return Err(bad_request("cwd must be a directory"));
    }

    Ok(cwd)
}

fn normalize_timeout(timeout_seconds: Option<u64>) -> u64 {
    timeout_seconds
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
        .clamp(1, MAX_TIMEOUT_SECONDS)
}

fn normalize_route_policy(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("suggested-execution")
        .to_string()
}

fn normalize_task_kind(raw: Option<&str>) -> String {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some("architecture") | Some("architect") => "architecture".to_string(),
        Some("implementation") | Some("implement") | Some("code") => "implementation".to_string(),
        Some("testing") | Some("test") | Some("bugfix") => "testing".to_string(),
        Some("documentation") | Some("docs") => "documentation".to_string(),
        Some(value) => value.to_string(),
        None => "general".to_string(),
    }
}

fn normalize_reasoning_level(raw: Option<&str>) -> String {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some("high") => "high".to_string(),
        Some("low") => "low".to_string(),
        Some("medium") => "medium".to_string(),
        Some(value) => value.to_string(),
        None => "medium".to_string(),
    }
}

fn normalize_runtime_mode_hint(raw: Option<&str>, task_kind: &str) -> String {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some("pane") => AGENT_RUNTIME_PANE.to_string(),
        Some("inline") => AGENT_RUNTIME_INLINE.to_string(),
        Some("background") => AGENT_RUNTIME_BACKGROUND.to_string(),
        Some(value) => value.to_string(),
        None if task_kind == "implementation" || task_kind == "testing" => {
            AGENT_RUNTIME_PANE.to_string()
        }
        None => AGENT_RUNTIME_BACKGROUND.to_string(),
    }
}

fn normalize_runtime_mode(
    raw: Option<&str>,
    wait_for_completion: bool,
) -> Result<String, (StatusCode, Json<ApiErrorResponse>)> {
    let mode = match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some("pane") => AGENT_RUNTIME_PANE.to_string(),
        Some("inline") => AGENT_RUNTIME_INLINE.to_string(),
        Some("background") => AGENT_RUNTIME_BACKGROUND.to_string(),
        Some(other) => {
            return Err(bad_request(format!(
                "runtimeMode must be one of '{AGENT_RUNTIME_INLINE}', '{AGENT_RUNTIME_BACKGROUND}', or '{AGENT_RUNTIME_PANE}', got '{other}'"
            )))
        }
        None if wait_for_completion => AGENT_RUNTIME_INLINE.to_string(),
        None => AGENT_RUNTIME_BACKGROUND.to_string(),
    };

    if mode == AGENT_RUNTIME_PANE && wait_for_completion {
        return Err(bad_request("pane runtime cannot be combined with waitForCompletion=true"));
    }

    Ok(mode)
}

fn normalize_callback_mode_for_run(
    raw: Option<&str>,
) -> Result<String, (StatusCode, Json<ApiErrorResponse>)> {
    Ok(normalize_callback_mode(raw)?.to_string())
}

fn normalize_callback_mode(
    raw: Option<&str>,
) -> Result<&'static str, (StatusCode, Json<ApiErrorResponse>)> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("auto") => Ok("auto"),
        Some("notify") => Ok("notify"),
        Some(_) => Err(bad_request("callbackMode must be 'auto' or 'notify'")),
    }
}

fn normalize_bridge_completion_status(
    raw: &str,
    timed_out: bool,
    cancelled: bool,
) -> Result<&'static str, (StatusCode, Json<ApiErrorResponse>)> {
    if timed_out {
        return Ok("timed_out");
    }
    if cancelled {
        return Ok("cancelled");
    }

    match raw {
        "succeeded" => Ok("succeeded"),
        "failed" => Ok("failed"),
        _ => Err(bad_request(
            "status must be 'succeeded', 'failed', 'timed_out', or 'cancelled'",
        )),
    }
}

fn authorize(
    headers: &HeaderMap,
    token: &str,
) -> Result<(), (StatusCode, Json<ApiErrorResponse>)> {
    let Some(header) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(unauthorized("Missing Authorization header"));
    };
    let Ok(value) = header.to_str() else {
        return Err(unauthorized("Authorization header must be valid UTF-8"));
    };
    let expected = format!("Bearer {token}");
    if value != expected {
        return Err(unauthorized("Invalid dispatch token"));
    }
    Ok(())
}

fn insert_dispatch_run(db: &Arc<Database>, record: DispatchRunRecord) -> Result<(), AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO dispatch_runs (
            run_id, target, provider_name, host_app, route_policy, task_kind, reasoning_level,
            runtime_mode, callback_mode, cwd, task_preview, status,
            timeout_seconds, started_at, updated_at, finished_at, exit_code, duration_ms,
            timed_out, cancelled, result_preview, result, stdout, stderr
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19,
            ?20, ?21, ?22, ?23, ?24
        )",
        params![
            record.run_id,
            record.target,
            record.provider_name,
            record.host_app,
            record.route_policy,
            record.task_kind,
            record.reasoning_level,
            record.runtime_mode,
            record.callback_mode,
            record.cwd,
            record.task_preview,
            record.status,
            record.timeout_seconds as i64,
            record.started_at,
            record.updated_at,
            record.finished_at,
            record.exit_code,
            record.duration_ms.map(|value| value as i64),
            if record.timed_out { 1 } else { 0 },
            if record.cancelled { 1 } else { 0 },
            record.result_preview,
            record.result,
            record.stdout,
            record.stderr,
        ],
    )
    .map_err(|err| AppError::Database(format!("写入 dispatch run 失败: {err}")))?;
    Ok(())
}

fn mark_dispatch_run_running(db: &Arc<Database>, run_id: &str) -> Result<(), AppError> {
    let updated_at = Utc::now().timestamp();
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "UPDATE dispatch_runs SET status = 'running', updated_at = ?2 WHERE run_id = ?1",
        params![run_id, updated_at],
    )
    .map_err(|err| AppError::Database(format!("更新 dispatch run 状态失败: {err}")))?;
    drop(conn);
    refresh_status_snapshot_from_db(db);
    Ok(())
}

fn append_dispatch_output_chunk(
    db: &Arc<Database>,
    run_id: &str,
    chunk: &str,
    stdout: bool,
) -> Result<(), AppError> {
    let updated_at = Utc::now().timestamp();
    let sql = if stdout {
        "UPDATE dispatch_runs
         SET stdout = substr(coalesce(stdout, '') || ?2, -?3), updated_at = ?4
         WHERE run_id = ?1"
    } else {
        "UPDATE dispatch_runs
         SET stderr = substr(coalesce(stderr, '') || ?2, -?3), updated_at = ?4
         WHERE run_id = ?1"
    };
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(sql, params![run_id, chunk, MAX_STORED_OUTPUT_CHARS, updated_at])
        .map_err(|err| AppError::Database(format!("追加 dispatch 输出失败: {err}")))?;
    drop(conn);
    refresh_status_snapshot_from_db(db);
    Ok(())
}

fn finalize_dispatch_run(db: &Arc<Database>, entry: &DispatchHistoryEntry) -> Result<(), AppError> {
    let finished_at = entry.timestamp;
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "UPDATE dispatch_runs
         SET status = ?2,
             updated_at = ?3,
             finished_at = ?4,
             exit_code = ?5,
             duration_ms = ?6,
             timed_out = ?7,
             cancelled = ?8,
             result_preview = ?9,
             result = ?10,
             stdout = ?11,
             stderr = ?12
         WHERE run_id = ?1",
        params![
            entry.run_id,
            entry.status,
            finished_at,
            finished_at,
            entry.exit_code,
            entry.duration_ms as i64,
            if entry.timed_out { 1 } else { 0 },
            if entry.cancelled { 1 } else { 0 },
            entry.result_preview,
            entry.result,
            entry.stdout,
            entry.stderr,
        ],
    )
    .map_err(|err| AppError::Database(format!("完成 dispatch run 失败: {err}")))?;
    Ok(())
}

fn reconcile_interrupted_dispatch_runs(db: &Arc<Database>) -> Result<(), AppError> {
    let finished_at = Utc::now().timestamp();
    let message = "Dispatch service restarted before this run completed.";
    let conn = crate::database::lock_conn!(db.conn);
    let affected = conn
        .execute(
            "UPDATE dispatch_runs
             SET status = 'failed',
                 updated_at = ?1,
                 finished_at = ?1,
                 duration_ms = CASE
                     WHEN started_at IS NULL THEN 0
                     WHEN ?1 > started_at THEN (?1 - started_at) * 1000
                     ELSE 0
                 END,
                 timed_out = 0,
                 cancelled = 0,
                 result_preview = ?2,
                 result = CASE
                     WHEN coalesce(result, '') = '' THEN ?3
                     ELSE result || '\n\n' || ?3
                 END
             WHERE status IN ('queued', 'running')",
            params![
                finished_at,
                truncate_preview(message),
                message,
            ],
        )
        .map_err(|err| AppError::Database(format!("恢复中断 dispatch run 失败: {err}")))?;
    if affected > 0 {
        log::info!("Recovered {affected} interrupted dispatch runs after restart");
    }
    Ok(())
}

fn load_dispatch_runs(
    db: &Arc<Database>,
    limit: usize,
    status_filter: Option<&str>,
) -> Result<Vec<DispatchRunRecord>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    let order_sql = "ORDER BY CASE WHEN status IN ('running','queued') THEN 0 ELSE 1 END, started_at DESC";
    let base_sql = "SELECT
            run_id, target, provider_name, host_app, route_policy, task_kind, reasoning_level,
            runtime_mode, callback_mode, cwd, task_preview, status,
            timeout_seconds, started_at, updated_at, finished_at, exit_code, duration_ms,
            timed_out, cancelled, result_preview, result, stdout, stderr
         FROM dispatch_runs";

    let runs = if let Some(status_filter) = status_filter.filter(|value| !value.trim().is_empty()) {
        let sql = format!("{base_sql} WHERE status = ?1 {order_sql} LIMIT ?2");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| AppError::Database(format!("查询 dispatch runs 失败: {err}")))?;
        let rows = stmt
            .query_map(params![status_filter, limit as i64], dispatch_run_from_row)
            .map_err(|err| AppError::Database(format!("读取 dispatch runs 失败: {err}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| AppError::Database(format!("解析 dispatch runs 失败: {err}")))?;
        rows
    } else {
        let sql = format!("{base_sql} {order_sql} LIMIT ?1");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| AppError::Database(format!("查询 dispatch runs 失败: {err}")))?;
        let rows = stmt
            .query_map(params![limit as i64], dispatch_run_from_row)
            .map_err(|err| AppError::Database(format!("读取 dispatch runs 失败: {err}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| AppError::Database(format!("解析 dispatch runs 失败: {err}")))?;
        rows
    };

    Ok(runs)
}

fn load_dispatch_run(db: &Arc<Database>, run_id: &str) -> Result<Option<DispatchRunRecord>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    conn.query_row(
        "SELECT
            run_id, target, provider_name, host_app, route_policy, task_kind, reasoning_level,
            runtime_mode, callback_mode, cwd, task_preview, status,
            timeout_seconds, started_at, updated_at, finished_at, exit_code, duration_ms,
            timed_out, cancelled, result_preview, result, stdout, stderr
         FROM dispatch_runs
         WHERE run_id = ?1",
        params![run_id],
        dispatch_run_from_row,
    )
    .optional()
    .map_err(|err| AppError::Database(format!("读取 dispatch run 失败: {err}")))
}

fn load_active_dispatch_runs(db: &Arc<Database>) -> Result<Vec<DispatchActiveRun>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    let mut stmt = conn
        .prepare(
            "SELECT run_id, started_at, target, provider_name, cwd, timeout_seconds
             FROM dispatch_runs
             WHERE status IN ('running', 'queued')
             ORDER BY CASE WHEN status = 'running' THEN 0 ELSE 1 END, started_at DESC",
        )
        .map_err(|err| AppError::Database(format!("查询运行中的 dispatch runs 失败: {err}")))?;

    let runs = stmt
        .query_map([], |row| {
        Ok(DispatchActiveRun {
            run_id: row.get(0)?,
            started_at: row.get(1)?,
            target: row.get(2)?,
            provider_name: row.get(3)?,
            cwd: row.get(4)?,
            timeout_seconds: row.get::<_, i64>(5)? as u64,
        })
    })
    .map_err(|err| AppError::Database(format!("读取运行中的 dispatch runs 失败: {err}")))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|err| AppError::Database(format!("解析运行中的 dispatch runs 失败: {err}")))?;
    Ok(runs)
}

fn load_latest_finished_dispatch_run(db: &Arc<Database>) -> Result<Option<DispatchStatusRun>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    conn.query_row(
        "SELECT
            run_id, updated_at, target, provider_name, cwd, timeout_seconds, status,
            timed_out, cancelled, exit_code, duration_ms, result_preview
         FROM dispatch_runs
         WHERE status NOT IN ('queued', 'running')
         ORDER BY updated_at DESC
         LIMIT 1",
        [],
        |row| {
            Ok(DispatchStatusRun {
                run_id: row.get(0)?,
                timestamp: row.get(1)?,
                target: row.get(2)?,
                provider_name: row.get(3)?,
                cwd: row.get(4)?,
                timeout_seconds: row.get::<_, i64>(5)? as u64,
                status: row.get(6)?,
                timed_out: row.get::<_, i64>(7)? != 0,
                cancelled: row.get::<_, i64>(8)? != 0,
                exit_code: row.get(9)?,
                duration_ms: row.get::<_, Option<i64>>(10)?.unwrap_or_default() as u64,
                result_preview: row.get(11)?,
            })
        },
    )
    .optional()
    .map_err(|err| AppError::Database(format!("读取最新 dispatch run 失败: {err}")))
}

fn refresh_status_snapshot_from_db(db: &Arc<Database>) {
    match build_status_snapshot_from_db(db) {
        Ok(snapshot) => write_status_snapshot(snapshot),
        Err(err) => log::warn!("Failed to refresh dispatch status snapshot: {err}"),
    }
}

fn build_status_snapshot_from_db(db: &Arc<Database>) -> Result<DispatchStatusSnapshot, AppError> {
    let running_runs = load_active_dispatch_runs(db)?;
    let last_run = match load_latest_finished_dispatch_run(db) {
        Ok(run) => run,
        Err(_) => load_last_history_summary(),
    };
    let state = if running_runs.is_empty() { "idle" } else { "running" }.to_string();
    let current_run = running_runs.first().cloned();
    Ok(DispatchStatusSnapshot {
        state,
        updated_at: Utc::now().timestamp(),
        running_runs,
        current_run,
        last_run,
    })
}

fn dispatch_run_from_row(row: &Row<'_>) -> rusqlite::Result<DispatchRunRecord> {
    Ok(DispatchRunRecord {
        run_id: row.get(0)?,
        target: row.get(1)?,
        provider_name: row.get(2)?,
        host_app: row.get(3)?,
        route_policy: row.get(4)?,
        task_kind: row.get(5)?,
        reasoning_level: row.get(6)?,
        runtime_mode: row.get(7)?,
        callback_mode: row.get(8)?,
        cwd: row.get(9)?,
        task_preview: row.get(10)?,
        status: row.get(11)?,
        timeout_seconds: row.get::<_, i64>(12)? as u64,
        started_at: row.get(13)?,
        updated_at: row.get(14)?,
        finished_at: row.get(15)?,
        exit_code: row.get(16)?,
        duration_ms: row.get::<_, Option<i64>>(17)?.map(|value| value as u64),
        timed_out: row.get::<_, i64>(18)? != 0,
        cancelled: row.get::<_, i64>(19)? != 0,
        result_preview: row.get(20)?,
        result: row.get(21)?,
        stdout: row.get(22)?,
        stderr: row.get(23)?,
    })
}

fn append_history(entry: DispatchHistoryEntry) {
    let path = get_app_config_dir().join(HISTORY_FILE_NAME);
    let line = match serde_json::to_string(&entry) {
        Ok(line) => line,
        Err(err) => {
            log::warn!("Failed to serialize dispatch history entry: {err}");
            return;
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!("Failed to create dispatch history directory: {err}");
            return;
        }
    }

    use std::io::Write;
    match fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            if let Err(err) = writeln!(file, "{line}") {
                log::warn!("Failed to append dispatch history: {err}");
            }
        }
        Err(err) => log::warn!("Failed to open dispatch history file: {err}"),
    }
}

fn status_path() -> PathBuf {
    get_app_config_dir().join(STATUS_FILE_NAME)
}

fn write_status_snapshot(snapshot: DispatchStatusSnapshot) {
    let path = status_path();
    let payload = match serde_json::to_vec_pretty(&snapshot) {
        Ok(payload) => payload,
        Err(err) => {
            log::warn!("Failed to serialize dispatch status snapshot: {err}");
            return;
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!("Failed to create dispatch status directory: {err}");
            return;
        }
    }

    if let Err(err) = fs::write(&path, payload) {
        log::warn!("Failed to write dispatch status snapshot: {err}");
    }
}

pub fn read_status_snapshot() -> Result<DispatchStatusSnapshot, AppError> {
    let path = status_path();
    if !path.exists() {
        return Ok(DispatchStatusSnapshot {
            state: "idle".to_string(),
            updated_at: Utc::now().timestamp(),
            running_runs: Vec::new(),
            current_run: None,
            last_run: load_last_history_summary(),
        });
    }

    let raw = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&raw)
        .map_err(|e| AppError::Message(format!("解析 Dispatch 状态文件失败: {e}")))
}

pub fn read_recent_runs(db: &Arc<Database>, limit: usize) -> Result<Vec<DispatchRunRecord>, AppError> {
    load_dispatch_runs(db, limit.clamp(1, 50), None)
}

pub fn plan_agent_task(
    db: &Arc<Database>,
    task: &str,
    policy: Option<&str>,
    target: Option<&str>,
    mode: Option<&str>,
) -> Result<AgentPlan, AppError> {
    let providers = collect_dispatch_providers(db, None)?;
    build_agent_plan(&providers, task, policy, target, mode)
}

fn load_last_history_summary() -> Option<DispatchStatusRun> {
    let path = get_app_config_dir().join(HISTORY_FILE_NAME);
    let raw = fs::read_to_string(path).ok()?;
    let line = raw.lines().rev().find(|line| !line.trim().is_empty())?;
    let entry: DispatchHistoryEntry = serde_json::from_str(line).ok()?;
    Some(DispatchStatusRun::from(entry))
}

fn truncate_preview(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 240 {
        return trimmed.to_string();
    }
    trimmed.chars().take(240).collect::<String>() + "..."
}

impl From<DispatchHistoryEntry> for DispatchStatusRun {
    fn from(entry: DispatchHistoryEntry) -> Self {
        Self {
            run_id: entry.run_id,
            timestamp: entry.timestamp,
            target: entry.target,
            provider_name: entry.provider_name,
            cwd: entry.cwd,
            timeout_seconds: entry.timeout_seconds,
            status: entry.status.to_string(),
            timed_out: entry.timed_out,
            cancelled: entry.cancelled,
            exit_code: entry.exit_code,
            duration_ms: entry.duration_ms,
            result_preview: entry.result_preview,
        }
    }
}

fn map_spawn_error(err: std::io::Error) -> AppError {
    if err.kind() == std::io::ErrorKind::NotFound {
        AppError::Message("Required CLI tool is not installed or not found in PATH".to_string())
    } else {
        AppError::Message(format!("启动子进程失败: {err}"))
    }
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiErrorResponse {
            error: message.into(),
        }),
    )
}

fn unauthorized(message: impl Into<String>) -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiErrorResponse {
            error: message.into(),
        }),
    )
}

fn internal_error(err: AppError) -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiErrorResponse {
            error: err.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        claude_dispatch_permission_mode, dispatch_name_slug, normalize_timeout, parse_target,
        preferred_dispatch_selector, provider_is_dispatchable, wrap_task_for_main_agent,
        DispatchHistoryEntry, DispatchStatusSnapshot, ParsedTarget, MAIN_AGENT_CALLBACK_TAG,
    };
    use crate::app_config::AppType;
    use crate::provider::Provider;
    use serde_json::json;

    #[test]
    fn parse_target_accepts_supported_apps() {
        let parsed = parse_target("claude:primary").expect("target should parse");
        assert_eq!(parsed.app, AppType::Claude);
        assert_eq!(parsed.provider_selector, "primary");

        let parsed = parse_target("codex:team").expect("target should parse");
        assert_eq!(parsed, ParsedTarget {
            app: AppType::Codex,
            provider_selector: "team".to_string(),
        });
    }

    #[test]
    fn parse_target_rejects_invalid_values() {
        assert!(parse_target("claude").is_err());
        assert!(parse_target("gemini:test").is_err());
        assert!(parse_target("claude:").is_err());
    }

    #[test]
    fn normalize_timeout_clamps_values() {
        assert_eq!(normalize_timeout(None), 120);
        assert_eq!(normalize_timeout(Some(0)), 1);
        assert_eq!(normalize_timeout(Some(9_999)), 900);
    }

    #[test]
    fn codex_provider_is_dispatchable_with_chatgpt_auth() {
        let settings = json!({
            "auth": {
                "OPENAI_API_KEY": null,
                "auth_mode": "chatgpt"
            },
            "config": "model = \"gpt-5.4\""
        });

        assert!(provider_is_dispatchable(&AppType::Codex, &settings));
    }

    #[test]
    fn claude_dispatch_defaults_to_bypass_permissions() {
        assert_eq!(claude_dispatch_permission_mode(&json!({})), "bypassPermissions");
    }

    #[test]
    fn claude_dispatch_honors_explicit_permission_mode() {
        let settings = json!({
            "dispatchPermissionMode": "dontAsk"
        });

        assert_eq!(claude_dispatch_permission_mode(&settings), "dontAsk");
    }

    #[test]
    fn wrapped_task_requires_main_agent_callback() {
        let wrapped = wrap_task_for_main_agent("Implement the feature.");

        assert!(wrapped.contains("main agent"));
        assert!(wrapped.contains(MAIN_AGENT_CALLBACK_TAG));
        assert!(wrapped.contains("message: 我已经实现完了"));
        assert!(wrapped.contains("Implement the feature."));
    }

    #[test]
    fn dispatch_name_slug_is_short_and_stable() {
        assert_eq!(dispatch_name_slug("DouBaoSeed"), "doubaoseed");
        assert_eq!(dispatch_name_slug("Kimi For Coding"), "kimi-for-coding");
        assert_eq!(dispatch_name_slug("Aliyun/CN"), "aliyun-cn");
    }

    #[test]
    fn preferred_dispatch_selector_prefers_alias() {
        let provider = Provider {
            id: "abc".to_string(),
            name: "DouBaoSeed".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
            alias: Some("db".to_string()),
        };

        assert_eq!(preferred_dispatch_selector(&provider), "db");
    }

    #[test]
    fn preferred_dispatch_selector_falls_back_to_id_when_slug_is_empty() {
        let provider = Provider {
            id: "aliyun-provider".to_string(),
            name: "////".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
            alias: None,
        };

        assert_eq!(preferred_dispatch_selector(&provider), "aliyun-provider");
    }

    #[test]
    fn legacy_history_entries_without_run_id_still_deserialize() {
        let entry: DispatchHistoryEntry = serde_json::from_value(json!({
            "timestamp": 1,
            "target": "codex:current",
            "providerName": "Codex",
            "cwd": "/tmp/project",
            "timeoutSeconds": 120,
            "status": "succeeded",
            "timedOut": false,
            "exitCode": 0,
            "durationMs": 1500,
            "resultPreview": "done",
            "result": "done",
            "stdout": "",
            "stderr": ""
        }))
        .expect("legacy history entry should deserialize");

        assert_eq!(entry.run_id, "");
    }

    #[test]
    fn legacy_status_snapshot_without_run_id_still_deserializes() {
        let snapshot: DispatchStatusSnapshot = serde_json::from_value(json!({
            "state": "idle",
            "updatedAt": 1,
            "currentRun": null,
            "lastRun": {
                "timestamp": 1,
                "target": "codex:current",
                "providerName": "Codex",
                "cwd": "/tmp/project",
                "timeoutSeconds": 120,
                "status": "succeeded",
                "timedOut": false,
                "exitCode": 0,
                "durationMs": 1500,
                "resultPreview": "done"
            }
        }))
        .expect("legacy status snapshot should deserialize");

        assert_eq!(snapshot.last_run.expect("last run").run_id, "");
    }
}
