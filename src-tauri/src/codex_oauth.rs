use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;

const OPENAI_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_OAUTH_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_OAUTH_CALLBACK_PORT: u16 = 1455;
const ACCESS_TOKEN_REFRESH_BUFFER_SECS: i64 = 300;
const LOGIN_SESSION_TIMEOUT_SECS: i64 = 600;
const OAUTH_SCOPE: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthConfig {
    #[serde(default = "default_chatgpt_auth_mode")]
    pub auth_mode: String,
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token_expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token_expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chatgpt_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthStatus {
    pub authenticated: bool,
    pub status: String,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub auth_provider: Option<String>,
    pub plan_type: Option<String>,
    pub expires_at: Option<i64>,
    pub id_token_expires_at: Option<i64>,
    pub last_refresh: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthRefreshResponse {
    pub settings_config: Value,
    pub status: CodexOAuthStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthLoginStart {
    pub session_id: String,
    pub authorize_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthLoginComplete {
    pub settings_config: Value,
    pub status: CodexOAuthStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthLoginPoll {
    pub pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CodexOAuthLoginComplete>,
}

#[derive(Debug, Clone)]
pub struct CodexOAuthLoginManager {
    sessions: Arc<Mutex<HashMap<String, LoginSessionState>>>,
}

#[derive(Debug, Clone)]
enum LoginSessionState {
    Pending { expires_at: i64 },
    Completed(CodexOAuthConfig),
    Failed(String),
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct JwtAuthClaims {
    #[serde(rename = "https://api.openai.com/auth")]
    auth: Option<JwtAuthSection>,
    #[serde(rename = "https://api.openai.com/profile")]
    profile: Option<JwtProfileSection>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    auth_provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwtAuthSection {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
    #[serde(default)]
    chatgpt_user_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwtProfileSection {
    #[serde(default)]
    email: Option<String>,
}

fn default_chatgpt_auth_mode() -> String {
    "chatgpt".to_string()
}

impl CodexOAuthLoginManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start_login(&self) -> Result<CodexOAuthLoginStart, AppError> {
        let session_id = Uuid::new_v4().to_string();
        let state_nonce = random_oauth_state();
        let verifier = random_pkce_verifier();
        let challenge = pkce_challenge(&verifier);
        let expires_at = (Utc::now() + Duration::seconds(LOGIN_SESSION_TIMEOUT_SECS)).timestamp();

        let listener = TcpListener::bind(("127.0.0.1", OPENAI_OAUTH_CALLBACK_PORT))
            .map_err(|e| AppError::Message(format!("启动 Codex OAuth 回调监听失败: {e}")))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| AppError::Message(format!("配置 Codex OAuth 监听失败: {e}")))?;
        let redirect_uri = format!(
            "http://localhost:{}/auth/callback",
            OPENAI_OAUTH_CALLBACK_PORT
        );
        let authorize_url = build_authorize_url(
            &redirect_uri,
            state_nonce.as_str(),
            challenge.as_str(),
        )?;

        self.sessions
            .lock()
            .map_err(|_| AppError::Message("Codex OAuth 会话锁已损坏".to_string()))?
            .insert(session_id.clone(), LoginSessionState::Pending { expires_at });

        let sessions = Arc::clone(&self.sessions);
        let listener_session_id = session_id.clone();
        thread::spawn(move || {
            run_login_listener(
                sessions,
                listener_session_id,
                listener,
                redirect_uri,
                state_nonce,
                verifier,
                expires_at,
            );
        });

        Ok(CodexOAuthLoginStart {
            session_id,
            authorize_url,
            expires_at,
        })
    }

    pub fn poll_login(
        &self,
        db: &Database,
        provider_id: Option<&str>,
        session_id: &str,
    ) -> Result<CodexOAuthLoginPoll, AppError> {
        let state = self
            .sessions
            .lock()
            .map_err(|_| AppError::Message("Codex OAuth 会话锁已损坏".to_string()))?
            .get(session_id)
            .cloned()
            .ok_or_else(|| AppError::Message("Codex OAuth 登录会话不存在或已过期".to_string()))?;

        match state {
            LoginSessionState::Pending { expires_at } => {
                if Utc::now().timestamp() >= expires_at {
                    self.sessions
                        .lock()
                        .map_err(|_| AppError::Message("Codex OAuth 会话锁已损坏".to_string()))?
                        .insert(
                            session_id.to_string(),
                            LoginSessionState::Failed("Codex OAuth 登录已超时，请重新登录".to_string()),
                        );
                    return Err(AppError::Message(
                        "Codex OAuth 登录已超时，请重新登录".to_string(),
                    ));
                }
                Ok(CodexOAuthLoginPoll {
                    pending: true,
                    result: None,
                })
            }
            LoginSessionState::Failed(error) => {
                self.sessions
                    .lock()
                    .map_err(|_| AppError::Message("Codex OAuth 会话锁已损坏".to_string()))?
                    .remove(session_id);
                Err(AppError::Message(error))
            }
            LoginSessionState::Completed(oauth) => {
                ensure_unique_account(db, provider_id, &oauth.account_id)?;
                self.sessions
                    .lock()
                    .map_err(|_| AppError::Message("Codex OAuth 会话锁已损坏".to_string()))?
                    .remove(session_id);

                let settings_config = codex_settings_with_oauth(&oauth);
                let status = codex_oauth_status(Some(&oauth));
                Ok(CodexOAuthLoginPoll {
                    pending: false,
                    result: Some(CodexOAuthLoginComplete {
                        settings_config,
                        status,
                    }),
                })
            }
        }
    }
}

pub fn codex_settings_with_oauth(oauth: &CodexOAuthConfig) -> Value {
    json!({
        "auth": build_auth_json(oauth),
        "oauth": oauth,
    })
}

pub fn codex_settings_merge_oauth(existing_settings: &Value, oauth: &CodexOAuthConfig) -> Value {
    let mut settings = existing_settings
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);
    settings.insert("auth".to_string(), build_auth_json(oauth));
    settings.insert(
        "oauth".to_string(),
        serde_json::to_value(oauth).unwrap_or(Value::Null),
    );
    Value::Object(settings)
}

pub fn codex_oauth_status_from_settings(settings: &Value) -> CodexOAuthStatus {
    codex_oauth_status(extract_oauth_config(settings).as_ref())
}

pub fn codex_oauth_status(oauth: Option<&CodexOAuthConfig>) -> CodexOAuthStatus {
    let Some(oauth) = oauth else {
        return CodexOAuthStatus {
            authenticated: false,
            status: "not_logged_in".to_string(),
            account_id: None,
            email: None,
            name: None,
            auth_provider: None,
            plan_type: None,
            expires_at: None,
            id_token_expires_at: None,
            last_refresh: None,
            last_error: None,
        };
    };

    let now = Utc::now().timestamp();
    let status = if oauth.invalid_reason.is_some() {
        "invalid"
    } else if oauth.account_id.trim().is_empty()
        || oauth.refresh_token.trim().is_empty()
        || oauth.access_token.trim().is_empty()
    {
        "invalid"
    } else if oauth
        .access_token_expires_at
        .is_some_and(|exp| exp <= now)
    {
        "expired"
    } else if oauth
        .access_token_expires_at
        .is_some_and(|exp| exp - now <= ACCESS_TOKEN_REFRESH_BUFFER_SECS)
    {
        "expiring"
    } else {
        "active"
    };

    CodexOAuthStatus {
        authenticated: oauth.invalid_reason.is_none() && !oauth.access_token.trim().is_empty(),
        status: status.to_string(),
        account_id: Some(oauth.account_id.clone()),
        email: oauth.email.clone(),
        name: oauth.name.clone(),
        auth_provider: oauth.auth_provider.clone(),
        plan_type: oauth.plan_type.clone(),
        expires_at: oauth.access_token_expires_at,
        id_token_expires_at: oauth.id_token_expires_at,
        last_refresh: oauth.last_refresh.clone(),
        last_error: oauth.invalid_reason.clone(),
    }
}

pub fn extract_oauth_config(settings: &Value) -> Option<CodexOAuthConfig> {
    serde_json::from_value(settings.get("oauth")?.clone()).ok()
}

pub fn ensure_unique_account(
    db: &Database,
    provider_id: Option<&str>,
    account_id: &str,
) -> Result<(), AppError> {
    if account_id.trim().is_empty() {
        return Err(AppError::Message("Codex OAuth 账号 ID 缺失".to_string()));
    }

    let providers = db.get_all_providers("codex")?;
    for provider in providers.values() {
        if provider_id.is_some_and(|id| id == provider.id) {
            continue;
        }
        let Some(existing) = extract_oauth_config(&provider.settings_config) else {
            continue;
        };
        if existing.account_id == account_id {
            return Err(AppError::Message(format!(
                "该 Codex 账号已绑定到 provider '{}' ({})",
                provider.name, provider.id
            )));
        }
    }

    Ok(())
}

pub fn refresh_oauth_settings_if_needed(
    settings: &Value,
    force_refresh: bool,
) -> Result<Value, AppError> {
    let Some(mut oauth) = extract_oauth_config(settings) else {
        return Ok(settings.clone());
    };

    let now = Utc::now().timestamp();
    let needs_refresh = force_refresh
        || oauth.invalid_reason.is_some()
        || oauth
            .access_token_expires_at
            .is_none_or(|exp| exp - now <= ACCESS_TOKEN_REFRESH_BUFFER_SECS)
        || oauth
            .id_token_expires_at
            .is_some_and(|exp| exp <= now + ACCESS_TOKEN_REFRESH_BUFFER_SECS);

    if !needs_refresh {
        let normalized = codex_settings_merge_oauth(settings, &oauth);
        return Ok(normalized);
    }

    let response = refresh_oauth_tokens(&oauth.refresh_token)?;
    oauth = oauth_from_token_response(response, Some(&oauth))?;
    oauth.invalid_reason = None;
    Ok(codex_settings_merge_oauth(settings, &oauth))
}

pub fn mark_oauth_invalid(settings: &Value, error: &str) -> Value {
    let Some(mut oauth) = extract_oauth_config(settings) else {
        return settings.clone();
    };
    oauth.invalid_reason = Some(error.to_string());
    codex_settings_merge_oauth(settings, &oauth)
}

pub fn persist_oauth_refresh_if_needed(
    db: &Database,
    provider: &Provider,
    force_refresh: bool,
) -> Result<Value, AppError> {
    let refreshed = match refresh_oauth_settings_if_needed(&provider.settings_config, force_refresh)
    {
        Ok(settings) => settings,
        Err(err) => {
            let invalid = mark_oauth_invalid(&provider.settings_config, &err.to_string());
            if invalid != provider.settings_config {
                db.update_provider_settings_config("codex", &provider.id, &invalid)?;
            }
            return Err(err);
        }
    };

    if refreshed != provider.settings_config {
        db.update_provider_settings_config("codex", &provider.id, &refreshed)?;
    }

    Ok(refreshed)
}

fn run_login_listener(
    sessions: Arc<Mutex<HashMap<String, LoginSessionState>>>,
    session_id: String,
    listener: TcpListener,
    redirect_uri: String,
    state_nonce: String,
    verifier: String,
    expires_at: i64,
) {
    let deadline = Instant::now() + StdDuration::from_secs(LOGIN_SESSION_TIMEOUT_SECS as u64);

    loop {
        if Instant::now() >= deadline || Utc::now().timestamp() >= expires_at {
            update_login_session(
                &sessions,
                &session_id,
                LoginSessionState::Failed("Codex OAuth 登录已超时，请重新登录".to_string()),
            );
            break;
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(StdDuration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(StdDuration::from_secs(5)));
                handle_login_request(
                    &sessions,
                    &session_id,
                    &mut stream,
                    &redirect_uri,
                    &state_nonce,
                    &verifier,
                );
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(StdDuration::from_millis(150));
            }
            Err(err) => {
                update_login_session(
                    &sessions,
                    &session_id,
                    LoginSessionState::Failed(format!("Codex OAuth 回调监听失败: {err}")),
                );
                break;
            }
        }
    }
}

fn handle_login_request(
    sessions: &Arc<Mutex<HashMap<String, LoginSessionState>>>,
    session_id: &str,
    stream: &mut TcpStream,
    redirect_uri: &str,
    state_nonce: &str,
    verifier: &str,
) {
    let outcome = parse_http_path(stream)
        .and_then(|path| process_login_path(&path, redirect_uri, state_nonce, verifier));

    match outcome {
        Ok(oauth) => {
            update_login_session(sessions, session_id, LoginSessionState::Completed(oauth));
            let _ = write_http_response(stream, true, "Codex OAuth 登录成功，现在可以回到 cc-switch 继续。", 200);
        }
        Err(err) => {
            update_login_session(
                sessions,
                session_id,
                LoginSessionState::Failed(err.to_string()),
            );
            let _ = write_http_response(
                stream,
                false,
                &format!("Codex OAuth 登录失败：{}", err),
                400,
            );
        }
    }
}

fn parse_http_path(stream: &mut TcpStream) -> Result<String, AppError> {
    let mut buf = [0u8; 8192];
    let size = stream
        .read(&mut buf)
        .map_err(|e| AppError::Message(format!("读取 Codex OAuth 回调失败: {e}")))?;
    if size == 0 {
        return Err(AppError::Message("Codex OAuth 回调请求为空".to_string()));
    }

    let request = String::from_utf8_lossy(&buf[..size]);
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| AppError::Message("Codex OAuth 回调请求无效".to_string()))?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method != "GET" || path.is_empty() {
        return Err(AppError::Message("Codex OAuth 回调请求无效".to_string()));
    }
    Ok(path.to_string())
}

fn process_login_path(
    path: &str,
    redirect_uri: &str,
    state_nonce: &str,
    verifier: &str,
) -> Result<CodexOAuthConfig, AppError> {
    let url = Url::parse(&format!("http://127.0.0.1{path}"))
        .map_err(|e| AppError::Message(format!("解析 Codex OAuth 回调失败: {e}")))?;

    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if let Some(error) = params.get("error") {
        let description = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| error.clone());
        return Err(AppError::Message(description));
    }

    let returned_state = params
        .get("state")
        .ok_or_else(|| AppError::Message("Codex OAuth 回调缺少 state".to_string()))?;
    if returned_state != state_nonce {
        return Err(AppError::Message(
            "Codex OAuth state 校验失败，请重试".to_string(),
        ));
    }

    let code = params
        .get("code")
        .ok_or_else(|| AppError::Message("Codex OAuth 回调缺少 code".to_string()))?;

    let response = exchange_authorization_code(code, redirect_uri, verifier)?;
    oauth_from_token_response(response, None)
}

fn write_http_response(
    stream: &mut TcpStream,
    success: bool,
    message: &str,
    status_code: u16,
) -> Result<(), AppError> {
    let title = if success { "Codex OAuth Success" } else { "Codex OAuth Failed" };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body><h2>{title}</h2><p>{message}</p></body></html>"
    );
    let status_text = if success { "OK" } else { "Bad Request" };
    let response = format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| AppError::Message(format!("写入 Codex OAuth 回调响应失败: {e}")))
}

fn update_login_session(
    sessions: &Arc<Mutex<HashMap<String, LoginSessionState>>>,
    session_id: &str,
    state: LoginSessionState,
) {
    if let Ok(mut guard) = sessions.lock() {
        guard.insert(session_id.to_string(), state);
    }
}

fn build_authorize_url(
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<String, AppError> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("response_type", "code");
    serializer.append_pair("client_id", OPENAI_OAUTH_CLIENT_ID);
    serializer.append_pair("redirect_uri", redirect_uri);
    serializer.append_pair("scope", OAUTH_SCOPE);
    serializer.append_pair("code_challenge", challenge);
    serializer.append_pair("code_challenge_method", "S256");
    serializer.append_pair("id_token_add_organizations", "true");
    serializer.append_pair("codex_cli_simplified_flow", "true");
    serializer.append_pair("state", state);
    serializer.append_pair("originator", "Codex Desktop");

    let query = serializer.finish().replace('+', "%20");
    let mut url = Url::parse(OPENAI_OAUTH_AUTHORIZE_URL)
        .map_err(|e| AppError::Message(format!("构建 Codex OAuth 登录地址失败: {e}")))?;
    url.set_query(Some(&query));
    Ok(url.into())
}

fn exchange_authorization_code(
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<OAuthTokenResponse, AppError> {
    let client = oauth_http_client()?;
    let response = client
        .post(OPENAI_OAUTH_TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", OPENAI_OAUTH_CLIENT_ID),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .send()
        .map_err(|e| AppError::Message(format!("Codex OAuth 换取 token 失败: {e}")))?;

    parse_token_response(response)
}

fn refresh_oauth_tokens(refresh_token: &str) -> Result<OAuthTokenResponse, AppError> {
    let client = oauth_http_client()?;
    let response = client
        .post(OPENAI_OAUTH_TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", OPENAI_OAUTH_CLIENT_ID),
            ("refresh_token", refresh_token),
            ("scope", OAUTH_SCOPE),
        ])
        .send()
        .map_err(|e| AppError::Message(format!("刷新 Codex OAuth token 失败: {e}")))?;

    parse_token_response(response)
}

fn parse_token_response(response: reqwest::blocking::Response) -> Result<OAuthTokenResponse, AppError> {
    let status = response.status();
    let text = response
        .text()
        .map_err(|e| AppError::Message(format!("读取 Codex OAuth 响应失败: {e}")))?;

    if !status.is_success() {
        return Err(AppError::Message(format!(
            "Codex OAuth 请求失败 ({status}): {text}"
        )));
    }

    serde_json::from_str::<OAuthTokenResponse>(&text)
        .map_err(|e| AppError::Message(format!("解析 Codex OAuth 响应失败: {e}")))
}

fn oauth_http_client() -> Result<Client, AppError> {
    Client::builder()
        .user_agent("cc-switch/3 Codex OAuth")
        .build()
        .map_err(|e| AppError::Message(format!("创建 Codex OAuth 客户端失败: {e}")))
}

fn oauth_from_token_response(
    response: OAuthTokenResponse,
    previous: Option<&CodexOAuthConfig>,
) -> Result<CodexOAuthConfig, AppError> {
    let access_claims = decode_jwt_claims(&response.access_token)?;
    let id_claims = response
        .id_token
        .as_deref()
        .map(decode_jwt_claims)
        .transpose()?;

    let account_id = access_claims
        .auth
        .as_ref()
        .and_then(|auth| auth.chatgpt_account_id.clone())
        .or_else(|| previous.map(|oauth| oauth.account_id.clone()))
        .ok_or_else(|| AppError::Message("Codex OAuth 响应缺少账号 ID".to_string()))?;

    let now = Utc::now();
    let access_expires_at = response
        .expires_in
        .map(|seconds| (now + Duration::seconds(seconds)).timestamp())
        .or(access_claims.exp)
        .or_else(|| previous.and_then(|oauth| oauth.access_token_expires_at));
    let id_expires_at = id_claims.as_ref().and_then(|claims| claims.exp).or_else(|| {
        response
            .id_token
            .as_deref()
            .and_then(jwt_exp_from_token)
            .or_else(|| previous.and_then(|oauth| oauth.id_token_expires_at))
    });

    let email = id_claims
        .as_ref()
        .and_then(|claims| claims.email.clone())
        .or_else(|| {
            access_claims
                .profile
                .as_ref()
                .and_then(|profile| profile.email.clone())
        })
        .or_else(|| access_claims.email.clone())
        .or_else(|| previous.and_then(|oauth| oauth.email.clone()));

    let name = id_claims
        .as_ref()
        .and_then(|claims| claims.name.clone())
        .or_else(|| access_claims.name.clone())
        .or_else(|| previous.and_then(|oauth| oauth.name.clone()));

    let auth_provider = id_claims
        .as_ref()
        .and_then(|claims| claims.auth_provider.clone())
        .or_else(|| previous.and_then(|oauth| oauth.auth_provider.clone()));

    let plan_type = access_claims
        .auth
        .as_ref()
        .and_then(|auth| auth.chatgpt_plan_type.clone())
        .or_else(|| previous.and_then(|oauth| oauth.plan_type.clone()));

    let chatgpt_user_id = access_claims
        .auth
        .as_ref()
        .and_then(|auth| auth.chatgpt_user_id.clone().or_else(|| auth.user_id.clone()))
        .or_else(|| previous.and_then(|oauth| oauth.chatgpt_user_id.clone()));

    Ok(CodexOAuthConfig {
        auth_mode: default_chatgpt_auth_mode(),
        account_id,
        access_token: response.access_token,
        refresh_token: response
            .refresh_token
            .or_else(|| previous.map(|oauth| oauth.refresh_token.clone()))
            .ok_or_else(|| AppError::Message("Codex OAuth 响应缺少 refresh token".to_string()))?,
        id_token: response.id_token.or_else(|| previous.and_then(|oauth| oauth.id_token.clone())),
        access_token_expires_at: access_expires_at,
        id_token_expires_at: id_expires_at,
        email,
        name,
        auth_provider,
        plan_type,
        chatgpt_user_id,
        last_refresh: Some(Utc::now().to_rfc3339()),
        invalid_reason: None,
    })
}

fn build_auth_json(oauth: &CodexOAuthConfig) -> Value {
    json!({
        "auth_mode": oauth.auth_mode,
        "OPENAI_API_KEY": Value::Null,
        "tokens": {
            "id_token": oauth.id_token,
            "access_token": oauth.access_token,
            "refresh_token": oauth.refresh_token,
            "account_id": oauth.account_id,
        },
        "last_refresh": oauth.last_refresh,
    })
}

fn decode_jwt_claims(token: &str) -> Result<JwtAuthClaims, AppError> {
    let payload = decode_jwt_payload(token)?;
    serde_json::from_value(payload)
        .map_err(|e| AppError::Message(format!("解析 Codex OAuth token claims 失败: {e}")))
}

fn jwt_exp_from_token(token: &str) -> Option<i64> {
    decode_jwt_payload(token)
        .ok()
        .and_then(|payload| payload.get("exp").and_then(Value::as_i64))
}

fn decode_jwt_payload(token: &str) -> Result<Value, AppError> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| AppError::Message("JWT token 格式无效".to_string()))?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| AppError::Message(format!("JWT payload 解码失败: {e}")))?;
    serde_json::from_slice::<Value>(&decoded)
        .map_err(|e| AppError::Message(format!("JWT payload 解析失败: {e}")))
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_oauth_state() -> String {
    random_digest_token("state")
}

fn random_pkce_verifier() -> String {
    random_digest_token("verifier")
}

fn random_digest_token(label: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(label.as_bytes());
    hasher.update(Uuid::new_v4().as_bytes());
    hasher.update(Uuid::new_v4().as_bytes());
    hasher.update(Utc::now().timestamp_nanos_opt().unwrap_or_default().to_le_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_oauth_settings_build_auth_snapshot() {
        let oauth = CodexOAuthConfig {
            account_id: "acct-1".to_string(),
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            id_token: Some("id".to_string()),
            access_token_expires_at: Some(Utc::now().timestamp() + 3600),
            id_token_expires_at: None,
            email: Some("user@example.com".to_string()),
            name: Some("User".to_string()),
            auth_provider: Some("google".to_string()),
            plan_type: Some("plus".to_string()),
            chatgpt_user_id: Some("user-1".to_string()),
            last_refresh: Some(Utc::now().to_rfc3339()),
            invalid_reason: None,
            auth_mode: default_chatgpt_auth_mode(),
        };

        let settings = codex_settings_with_oauth(&oauth);
        assert_eq!(
            settings
                .get("auth")
                .and_then(|auth| auth.get("tokens"))
                .and_then(|tokens| tokens.get("account_id"))
                .and_then(Value::as_str),
            Some("acct-1")
        );
        assert!(settings.get("oauth").is_some());
    }

    #[test]
    fn oauth_status_reports_expiring() {
        let oauth = CodexOAuthConfig {
            account_id: "acct-1".to_string(),
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            id_token: None,
            access_token_expires_at: Some(Utc::now().timestamp() + 120),
            id_token_expires_at: None,
            email: None,
            name: None,
            auth_provider: None,
            plan_type: None,
            chatgpt_user_id: None,
            last_refresh: None,
            invalid_reason: None,
            auth_mode: default_chatgpt_auth_mode(),
        };

        assert_eq!(codex_oauth_status(Some(&oauth)).status, "expiring");
    }

    #[test]
    fn oauth_status_reports_invalid_reason() {
        let oauth = CodexOAuthConfig {
            account_id: "acct-1".to_string(),
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            id_token: None,
            access_token_expires_at: Some(Utc::now().timestamp() + 3600),
            id_token_expires_at: None,
            email: None,
            name: None,
            auth_provider: None,
            plan_type: None,
            chatgpt_user_id: None,
            last_refresh: None,
            invalid_reason: Some("boom".to_string()),
            auth_mode: default_chatgpt_auth_mode(),
        };

        assert_eq!(codex_oauth_status(Some(&oauth)).status, "invalid");
    }
}
