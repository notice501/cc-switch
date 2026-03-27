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
use uuid::Uuid;

use crate::app_config::AppType;
use crate::config::get_app_config_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::build_effective_settings_with_common_config;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const MAX_TIMEOUT_SECONDS: u64 = 900;
const HISTORY_FILE_NAME: &str = "dispatch-history.jsonl";
const DISCOVERY_FILE_NAME: &str = "dispatch-api.json";

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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchRunResponse {
    ok: bool,
    target: String,
    provider_name: String,
    status: &'static str,
    timed_out: bool,
    exit_code: Option<i32>,
    duration_ms: u128,
    cwd: String,
    result: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchHistoryEntry {
    timestamp: i64,
    target: String,
    provider_name: String,
    cwd: String,
    timeout_seconds: u64,
    status: &'static str,
    timed_out: bool,
    exit_code: Option<i32>,
    duration_ms: u128,
    result_preview: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTarget {
    app: AppType,
    provider_id: String,
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

    let router = Router::new()
        .route("/health", get(health_check))
        .route("/v1/dispatch/providers", get(list_dispatch_providers))
        .route("/v1/dispatch/run", post(run_dispatch))
        .with_state(DispatchApiState {
            db,
            token: Arc::new(discovery.token.clone()),
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

    if request.task.trim().is_empty() {
        return Err(bad_request("Task content cannot be empty"));
    }

    let provider = load_effective_provider(&state.db, &parsed_target).map_err(internal_error)?;
    let provider_name = provider.name.clone();

    let runner = match parsed_target.app {
        AppType::Claude => RunnerKind::Claude,
        AppType::Codex => RunnerKind::Codex,
        _ => return Err(bad_request("Only Claude and Codex targets are supported")),
    };

    let run_result = runner
        .run(&provider, &cwd, request.task.trim(), timeout_seconds)
        .await;

    let output = match run_result {
        Ok(output) => output,
        Err(err) => {
            let error_message = err.to_string();
            let failure = RunnerOutput {
                status: "failed",
                timed_out: false,
                exit_code: None,
                duration_ms: 0,
                result: error_message.clone(),
                stdout: String::new(),
                stderr: error_message.clone(),
            };
            append_history(DispatchHistoryEntry {
                timestamp: Utc::now().timestamp(),
                target: request.target.clone(),
                provider_name,
                cwd: cwd.display().to_string(),
                timeout_seconds,
                status: failure.status,
                timed_out: failure.timed_out,
                exit_code: failure.exit_code,
                duration_ms: failure.duration_ms,
                result_preview: truncate_preview(&failure.result),
                stdout: failure.stdout.clone(),
                stderr: failure.stderr.clone(),
            });
            return Err(internal_error(err));
        }
    };

    append_history(DispatchHistoryEntry {
        timestamp: Utc::now().timestamp(),
        target: request.target.clone(),
        provider_name: provider.name.clone(),
        cwd: cwd.display().to_string(),
        timeout_seconds,
        status: output.status,
        timed_out: output.timed_out,
        exit_code: output.exit_code,
        duration_ms: output.duration_ms,
        result_preview: truncate_preview(&output.result),
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
    });

    Ok(Json(DispatchRunResponse {
        ok: output.status == "succeeded",
        target: request.target,
        provider_name: provider.name,
        status: output.status,
        timed_out: output.timed_out,
        exit_code: output.exit_code,
        duration_ms: output.duration_ms,
        cwd: cwd.display().to_string(),
        result: output.result,
        stdout: output.stdout,
        stderr: output.stderr,
    }))
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

    let mut command = Command::new("claude");
    command
        .current_dir(cwd)
        .arg("-p")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg("--disable-slash-commands")
        .arg("--no-session-persistence")
        .arg("--add-dir")
        .arg(cwd)
        .arg("--")
        .arg(task)
        .kill_on_drop(true);

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

async fn run_codex(
    provider: &Provider,
    cwd: &Path,
    task: &str,
    timeout_seconds: u64,
) -> Result<RunnerOutput, AppError> {
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

    let mut command = Command::new("codex");
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
        .arg(task)
        .kill_on_drop(true)
        .env("HOME", temp_home.path())
        .env("CODEX_HOME", &codex_dir)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL");

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
        for provider in all.values() {
            let effective_settings = build_effective_settings_with_common_config(db, &app, provider)?;
            if !provider_is_dispatchable(&app, &effective_settings) {
                continue;
            }
            providers.push(DispatchProviderTarget {
                target: format!("{}:{}", app.as_str(), provider.id),
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

fn provider_is_dispatchable(app: &AppType, settings: &Value) -> bool {
    match app {
        AppType::Claude => extract_claude_env(settings).is_ok(),
        AppType::Codex => settings
            .get("auth")
            .and_then(Value::as_object)
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(Value::as_str)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            && settings
                .get("config")
                .and_then(Value::as_str)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
        _ => false,
    }
}

fn load_effective_provider(db: &Arc<Database>, target: &ParsedTarget) -> Result<Provider, AppError> {
    let provider = db
        .get_provider_by_id(&target.provider_id, target.app.as_str())?
        .ok_or_else(|| {
            AppError::Message(format!(
                "Dispatch target '{}' does not exist in cc-switch",
                format!("{}:{}", target.app.as_str(), target.provider_id)
            ))
        })?;

    let mut effective = provider.clone();
    effective.settings_config = build_effective_settings_with_common_config(db, &target.app, &provider)?;
    Ok(effective)
}

fn parse_dispatchable_app(raw: &str) -> Result<AppType, (StatusCode, Json<ApiErrorResponse>)> {
    match raw {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        _ => Err(bad_request("Only 'claude' and 'codex' providers are supported")),
    }
}

fn parse_target(target: &str) -> Result<ParsedTarget, (StatusCode, Json<ApiErrorResponse>)> {
    let Some((app, provider_id)) = target.split_once(':') else {
        return Err(bad_request(
            "Target must use the form 'claude:<provider_id>' or 'codex:<provider_id>'",
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

    let provider_id = provider_id.trim();
    if provider_id.is_empty() {
        return Err(bad_request("Target provider id cannot be empty"));
    }

    Ok(ParsedTarget {
        app,
        provider_id: provider_id.to_string(),
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

fn truncate_preview(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 240 {
        return trimmed.to_string();
    }
    trimmed.chars().take(240).collect::<String>() + "..."
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
    use super::{normalize_timeout, parse_target, ParsedTarget};
    use crate::app_config::AppType;

    #[test]
    fn parse_target_accepts_supported_apps() {
        let parsed = parse_target("claude:primary").expect("target should parse");
        assert_eq!(parsed.app, AppType::Claude);
        assert_eq!(parsed.provider_id, "primary");

        let parsed = parse_target("codex:team").expect("target should parse");
        assert_eq!(parsed, ParsedTarget {
            app: AppType::Codex,
            provider_id: "team".to_string(),
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
}
