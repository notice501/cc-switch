use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{oneshot, Mutex};
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
    active_run_id: Arc<Mutex<Option<String>>>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchRunResponse {
    accepted: bool,
    completed: bool,
    run_id: String,
    target: String,
    provider_name: String,
    state: &'static str,
    started_at: i64,
    timeout_seconds: u64,
    cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timed_out: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u128>,
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
    exit_code: Option<i32>,
    duration_ms: u128,
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
    exit_code: Option<i32>,
    duration_ms: u128,
    result_preview: String,
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
    exit_code: Option<i32>,
    duration_ms: u128,
    result: String,
    stdout: String,
    stderr: String,
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
    initialize_status_snapshot();

    let router = Router::new()
        .route("/health", get(health_check))
        .route("/v1/dispatch/providers", get(list_dispatch_providers))
        .route("/v1/dispatch/run", post(run_dispatch))
        .with_state(DispatchApiState {
            db,
            token: Arc::new(discovery.token.clone()),
            active_run_id: Arc::new(Mutex::new(None)),
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

    claim_active_run(&state.active_run_id, &run_id).await?;

    write_status_snapshot(DispatchStatusSnapshot {
        state: "running".to_string(),
        updated_at: started_at,
        current_run: Some(DispatchActiveRun {
            run_id: run_id.clone(),
            started_at,
            target: request.target.clone(),
            provider_name: provider_name.clone(),
            cwd: cwd_display.clone(),
            timeout_seconds,
        }),
        last_run: load_last_history_summary(),
    });
    let (tx, rx) = oneshot::channel();
    let active_run_id = state.active_run_id.clone();
    let target = request.target.clone();
    let provider_name_for_task = provider.name.clone();
    let run_id_for_task = run_id.clone();
    let cwd_for_task = cwd.clone();

    tauri::async_runtime::spawn(async move {
        let response = execute_dispatch_run(
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
        release_active_run(&active_run_id, &run_id_for_task).await;
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

impl RunnerKind {
    async fn run(
        self,
        provider: &Provider,
        cwd: &Path,
        task: &str,
        timeout_seconds: u64,
    ) -> Result<RunnerOutput, AppError> {
        match self {
            RunnerKind::Claude => run_claude(provider, cwd, task, timeout_seconds).await,
            RunnerKind::Codex => run_codex(provider, cwd, task, timeout_seconds).await,
        }
    }
}

async fn run_claude(
    provider: &Provider,
    cwd: &Path,
    task: &str,
    timeout_seconds: u64,
) -> Result<RunnerOutput, AppError> {
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

    run_subprocess(command, timeout_seconds, None).await
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

async fn run_codex(
    provider: &Provider,
    cwd: &Path,
    task: &str,
    timeout_seconds: u64,
) -> Result<RunnerOutput, AppError> {
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

    run_subprocess(command, timeout_seconds, Some(last_message_path)).await
}

async fn run_subprocess(
    mut command: Command,
    timeout_seconds: u64,
    last_message_path: Option<PathBuf>,
) -> Result<RunnerOutput, AppError> {
    let started = std::time::Instant::now();
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let child = command.spawn().map_err(map_spawn_error)?;

    let output = tokio::time::timeout(Duration::from_secs(timeout_seconds), child.wait_with_output())
        .await;

    match output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let exit_code = output.status.code();
            let duration_ms = started.elapsed().as_millis();

            let mut result = last_message_path
                .as_ref()
                .and_then(|path| fs::read_to_string(path).ok())
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| stdout.clone());

            if result.is_empty() {
                result = stderr.clone();
            }
            if result.is_empty() {
                result = "Command completed without output.".to_string();
            }

            let status = if output.status.success() {
                "succeeded"
            } else {
                "failed"
            };

            Ok(RunnerOutput {
                status,
                timed_out: false,
                exit_code,
                duration_ms,
                result,
                stdout,
                stderr,
            })
        }
        Ok(Err(err)) => Err(AppError::Message(format!("等待子进程结果失败: {err}"))),
        Err(_) => {
            let duration_ms = started.elapsed().as_millis();
            Ok(RunnerOutput {
                status: "timed_out",
                timed_out: true,
                exit_code: None,
                duration_ms,
                result: format!("Dispatch request timed out after {timeout_seconds} seconds."),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

async fn execute_dispatch_run(
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
    let output = match runner.run(&provider, &cwd, &task, timeout_seconds).await {
        Ok(output) => output,
        Err(err) => failure_output_from_error(err),
    };
    let cwd_display = cwd.display().to_string();
    let history_entry = history_entry_from_output(
        run_id.clone(),
        target.clone(),
        provider_name.clone(),
        cwd_display.clone(),
        timeout_seconds,
        output,
    );
    append_history(history_entry.clone());
    write_status_snapshot(finished_status_snapshot(&history_entry));

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

async fn claim_active_run(
    active_run_id: &Arc<Mutex<Option<String>>>,
    run_id: &str,
) -> Result<(), (StatusCode, Json<ApiErrorResponse>)> {
    let mut current = active_run_id.lock().await;
    if let Some(existing) = current.as_ref() {
        return Err(conflict(format!(
            "A dispatch run is already active (`{existing}`). Use `/dispatch-task status` to inspect it."
        )));
    }
    *current = Some(run_id.to_string());
    Ok(())
}

async fn release_active_run(active_run_id: &Arc<Mutex<Option<String>>>, run_id: &str) {
    let mut current = active_run_id.lock().await;
    if current.as_deref() == Some(run_id) {
        *current = None;
    }
}

fn failure_output_from_error(err: AppError) -> RunnerOutput {
    let error_message = err.to_string();
    RunnerOutput {
        status: "failed",
        timed_out: false,
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
            state: "running",
            started_at,
            timeout_seconds,
            cwd,
            ok: None,
            status: None,
            timed_out: None,
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
        } else {
            "failed"
        };

        Self {
            accepted: true,
            completed: true,
            run_id,
            target,
            provider_name,
            state: "finished",
            started_at,
            timeout_seconds,
            cwd,
            ok: Some(result_status == "succeeded"),
            status: Some(result_status),
            timed_out: Some(history_entry.timed_out),
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

fn initialize_status_snapshot() {
    write_status_snapshot(DispatchStatusSnapshot {
        state: "idle".to_string(),
        updated_at: Utc::now().timestamp(),
        current_run: None,
        last_run: load_last_history_summary(),
    });
}

pub fn read_status_snapshot() -> Result<DispatchStatusSnapshot, AppError> {
    let path = status_path();
    if !path.exists() {
        return Ok(DispatchStatusSnapshot {
            state: "idle".to_string(),
            updated_at: Utc::now().timestamp(),
            current_run: None,
            last_run: load_last_history_summary(),
        });
    }

    let raw = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&raw)
        .map_err(|e| AppError::Message(format!("解析 Dispatch 状态文件失败: {e}")))
}

fn load_last_history_summary() -> Option<DispatchStatusRun> {
    let path = get_app_config_dir().join(HISTORY_FILE_NAME);
    let raw = fs::read_to_string(path).ok()?;
    let line = raw.lines().rev().find(|line| !line.trim().is_empty())?;
    let entry: DispatchHistoryEntry = serde_json::from_str(line).ok()?;
    Some(DispatchStatusRun::from(entry))
}

fn finished_status_snapshot(entry: &DispatchHistoryEntry) -> DispatchStatusSnapshot {
    DispatchStatusSnapshot {
        state: "idle".to_string(),
        updated_at: Utc::now().timestamp(),
        current_run: None,
        last_run: Some(DispatchStatusRun::from(entry.clone())),
    }
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

fn conflict(message: impl Into<String>) -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::CONFLICT,
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
