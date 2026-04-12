//! Usage script execution
//!
//! Handles executing and formatting usage query results.

use crate::app_config::AppType;
use crate::codex_oauth::{extract_oauth_config, persist_oauth_refresh_if_needed};
use crate::error::AppError;
use crate::provider::{Provider, UsageData, UsageResult, UsageScript};
use crate::settings;
use crate::store::AppState;
use crate::usage_script;
use chrono::{TimeZone, Utc};
use serde_json::Value;

const TEMPLATE_TYPE_CODEX_CHATGPT_OAUTH: &str = "codex_chatgpt_oauth";
const CODEX_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_USAGE_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

/// Execute usage script and format result (private helper method)
pub(crate) async fn execute_and_format_usage_result(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    match usage_script::execute_usage_script(
        script_code,
        api_key,
        base_url,
        timeout,
        access_token,
        user_id,
        template_type,
    )
    .await
    {
        Ok(data) => {
            let usage_list: Vec<UsageData> = if data.is_array() {
                serde_json::from_value(data).map_err(|e| {
                    AppError::localized(
                        "usage_script.data_format_error",
                        format!("数据格式错误: {e}"),
                        format!("Data format error: {e}"),
                    )
                })?
            } else {
                let single: UsageData = serde_json::from_value(data).map_err(|e| {
                    AppError::localized(
                        "usage_script.data_format_error",
                        format!("数据格式错误: {e}"),
                        format!("Data format error: {e}"),
                    )
                })?;
                vec![single]
            };

            Ok(UsageResult {
                success: true,
                data: Some(usage_list),
                error: None,
            })
        }
        Err(err) => {
            let lang = settings::get_settings()
                .language
                .unwrap_or_else(|| "zh".to_string());

            let msg = match err {
                AppError::Localized { zh, en, .. } => {
                    if lang == "en" {
                        en
                    } else {
                        zh
                    }
                }
                other => other.to_string(),
            };

            Ok(UsageResult {
                success: false,
                data: None,
                error: Some(msg),
            })
        }
    }
}

/// Extract API key from provider configuration
fn extract_api_key_from_provider(provider: &crate::provider::Provider) -> Option<String> {
    if let Some(env) = provider.settings_config.get("env") {
        // Try multiple possible API key fields
        env.get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .or_else(|| env.get("OPENROUTER_API_KEY"))
            .or_else(|| env.get("GOOGLE_API_KEY"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// Extract base URL from provider configuration
fn extract_base_url_from_provider(provider: &crate::provider::Provider) -> Option<String> {
    if let Some(env) = provider.settings_config.get("env") {
        // Try multiple possible base URL fields
        env.get("ANTHROPIC_BASE_URL")
            .or_else(|| env.get("GOOGLE_GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string())
    } else {
        None
    }
}

fn provider_supports_builtin_codex_usage(app_type: AppType, provider: &Provider) -> bool {
    matches!(app_type, AppType::Codex) && extract_oauth_config(&provider.settings_config).is_some()
}

fn provider_uses_builtin_codex_usage(
    app_type: AppType,
    provider: &Provider,
    usage_script: Option<&UsageScript>,
) -> bool {
    if !provider_supports_builtin_codex_usage(app_type, provider) {
        return false;
    }

    match usage_script.and_then(|script| script.template_type.as_deref()) {
        Some(TEMPLATE_TYPE_CODEX_CHATGPT_OAUTH) => true,
        Some(_) => false,
        None => true,
    }
}

async fn fetch_codex_oauth_usage(
    provider: &Provider,
    endpoint_override: Option<&str>,
) -> Result<UsageResult, AppError> {
    let oauth = extract_oauth_config(&provider.settings_config).ok_or_else(|| {
        AppError::localized(
            "provider.usage.codex_oauth_missing",
            "当前 Codex Provider 未绑定 OAuth 账号",
            "This Codex provider is not bound to an OAuth account",
        )
    })?;

    if oauth.access_token.trim().is_empty() || oauth.account_id.trim().is_empty() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some("Codex OAuth 凭证不完整".to_string()),
        });
    }

    let endpoint = endpoint_override
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(CODEX_USAGE_ENDPOINT);
    let response = reqwest::Client::new()
        .get(endpoint)
        .header("authorization", format!("Bearer {}", oauth.access_token))
        .header("chatgpt-account-id", oauth.account_id)
        .header("user-agent", CODEX_USAGE_USER_AGENT)
        .send()
        .await
        .map_err(|e| {
            AppError::localized(
                "provider.usage.codex_oauth_request_failed",
                format!("Codex 用量请求失败: {e}"),
                format!("Codex usage request failed: {e}"),
            )
        })?;

    let status = response.status();
    let body = response.text().await.map_err(|e| {
        AppError::localized(
            "provider.usage.codex_oauth_response_failed",
            format!("读取 Codex 用量响应失败: {e}"),
            format!("Failed to read Codex usage response: {e}"),
        )
    })?;

    if !status.is_success() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some(format!("Codex usage API returned {status}: {body}")),
        });
    }

    let json: Value = serde_json::from_str(&body).map_err(|e| {
        AppError::localized(
            "provider.usage.codex_oauth_parse_failed",
            format!("解析 Codex 用量响应失败: {e}"),
            format!("Failed to parse Codex usage response: {e}"),
        )
    })?;

    let data = parse_codex_usage_data(&json);
    Ok(UsageResult {
        success: true,
        data: Some(data),
        error: None,
    })
}

fn parse_codex_usage_data(root: &Value) -> Vec<UsageData> {
    let plan_type = root
        .get("plan_type")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let mut results = Vec::new();

    if let Some(rate_limit) = root.get("rate_limit").and_then(Value::as_object) {
        if let Some(primary) = rate_limit.get("primary_window") {
            if let Some(item) = parse_codex_usage_window("5h", primary, plan_type.as_deref()) {
                results.push(item);
            }
        }
        if let Some(secondary) = rate_limit.get("secondary_window") {
            if let Some(item) = parse_codex_usage_window("weekly", secondary, plan_type.as_deref())
            {
                results.push(item);
            }
        }
    }

    if let Some(credits) = root.get("credits").and_then(Value::as_object) {
        let balance = credits
            .get("balance")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok());
        let has_credits = credits
            .get("has_credits")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let unlimited = credits
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if has_credits || unlimited || balance.is_some() {
            let extra = if unlimited {
                Some("Unlimited credits".to_string())
            } else {
                None
            };
            let unit = if balance.is_some() {
                Some("USD".to_string())
            } else {
                None
            };
            results.push(UsageData {
                plan_name: plan_type
                    .as_ref()
                    .map(|plan| format!("{plan} credits"))
                    .or_else(|| Some("credits".to_string())),
                extra,
                is_valid: Some(true),
                invalid_message: None,
                total: None,
                used: None,
                remaining: balance,
                unit,
            });
        }
    }

    if results.is_empty() {
        results.push(UsageData {
            plan_name: plan_type,
            extra: None,
            is_valid: Some(false),
            invalid_message: Some("No usable usage windows found".to_string()),
            total: None,
            used: None,
            remaining: None,
            unit: None,
        });
    }

    results
}

fn parse_codex_usage_window(
    label: &str,
    window: &Value,
    plan_type: Option<&str>,
) -> Option<UsageData> {
    let window_obj = window.as_object()?;
    let used = match window_obj.get("used_percent") {
        Some(Value::Number(num)) => num.as_f64(),
        _ => None,
    }?;
    let remaining = (100.0 - used).max(0.0);
    let reset_at = window_obj
        .get("reset_at")
        .and_then(Value::as_i64)
        .and_then(format_codex_usage_reset_at);

    Some(UsageData {
        plan_name: Some(match plan_type {
            Some(plan) if !plan.trim().is_empty() => format!("{plan} {label}"),
            _ => label.to_string(),
        }),
        extra: reset_at.map(|text| format!("Reset: {text}")),
        is_valid: Some(true),
        invalid_message: None,
        total: Some(100.0),
        used: Some(used),
        remaining: Some(remaining),
        unit: Some("%".to_string()),
    })
}

fn format_codex_usage_reset_at(timestamp: i64) -> Option<String> {
    Utc.timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
}

/// Query provider usage (using saved script configuration)
pub async fn query_usage(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<UsageResult, AppError> {
    let (script_code, timeout, api_key, base_url, access_token, user_id, template_type) = {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers.get(provider_id).ok_or_else(|| {
            AppError::localized(
                "provider.not_found",
                format!("供应商不存在: {provider_id}"),
                format!("Provider not found: {provider_id}"),
            )
        })?;

        let usage_script = provider.meta.as_ref().and_then(|m| m.usage_script.as_ref());

        if usage_script.is_some_and(|script| !script.enabled) {
            return Err(AppError::localized(
                "provider.usage.disabled",
                "用量查询未启用",
                "Usage query is disabled",
            ));
        }

        if provider_uses_builtin_codex_usage(app_type.clone(), provider, usage_script) {
            let effective_provider = persist_oauth_refresh_if_needed(state.db.as_ref(), provider, false)?;
            let refreshed_provider = Provider {
                settings_config: effective_provider,
                ..provider.clone()
            };
            return fetch_codex_oauth_usage(
                &refreshed_provider,
                usage_script.and_then(|script| script.base_url.as_deref()),
            )
            .await;
        }

        let usage_script = usage_script.ok_or_else(|| {
            AppError::localized(
                "provider.usage.script.missing",
                "未配置用量查询脚本",
                "Usage script is not configured",
            )
        })?;

        // Get credentials: prioritize UsageScript values, fallback to provider config
        let api_key = usage_script
            .api_key
            .clone()
            .filter(|k| !k.is_empty())
            .or_else(|| extract_api_key_from_provider(provider))
            .unwrap_or_default();

        let base_url = usage_script
            .base_url
            .clone()
            .filter(|u| !u.is_empty())
            .or_else(|| extract_base_url_from_provider(provider))
            .unwrap_or_default();

        (
            usage_script.code.clone(),
            usage_script.timeout.unwrap_or(10),
            api_key,
            base_url,
            usage_script.access_token.clone(),
            usage_script.user_id.clone(),
            usage_script.template_type.clone(),
        )
    };

    execute_and_format_usage_result(
        &script_code,
        &api_key,
        &base_url,
        timeout,
        access_token.as_deref(),
        user_id.as_deref(),
        template_type.as_deref(),
    )
    .await
}

/// Test usage script (using temporary script content, not saved)
#[allow(clippy::too_many_arguments)]
pub async fn test_usage_script(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    script_code: &str,
    timeout: u64,
    api_key: Option<&str>,
    base_url: Option<&str>,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    if template_type == Some(TEMPLATE_TYPE_CODEX_CHATGPT_OAUTH) {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers.get(provider_id).ok_or_else(|| {
            AppError::localized(
                "provider.not_found",
                format!("供应商不存在: {provider_id}"),
                format!("Provider not found: {provider_id}"),
            )
        })?;

        if !provider_supports_builtin_codex_usage(app_type, provider) {
            return Err(AppError::localized(
                "provider.usage.codex_oauth_missing",
                "当前 Codex Provider 未绑定 OAuth 账号",
                "This Codex provider is not bound to an OAuth account",
            ));
        }

        return fetch_codex_oauth_usage(provider, base_url).await;
    }

    // Use provided credential parameters directly for testing
    execute_and_format_usage_result(
        script_code,
        api_key.unwrap_or(""),
        base_url.unwrap_or(""),
        timeout,
        access_token,
        user_id,
        template_type,
    )
    .await
}

/// Validate UsageScript configuration (boundary checks)
pub(crate) fn validate_usage_script(script: &UsageScript) -> Result<(), AppError> {
    // Validate auto query interval (0-1440 minutes, max 24 hours)
    if let Some(interval) = script.auto_query_interval {
        if interval > 1440 {
            return Err(AppError::localized(
                "usage_script.interval_too_large",
                format!("自动查询间隔不能超过 1440 分钟（24小时），当前值: {interval}"),
                format!(
                    "Auto query interval cannot exceed 1440 minutes (24 hours), current: {interval}"
                ),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_codex_usage_data_maps_windows_and_credits() {
        let data = parse_codex_usage_data(&json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 81.5,
                    "limit_window_seconds": 18000,
                    "reset_at": 1760000000
                },
                "secondary_window": {
                    "used_percent": 12.0,
                    "limit_window_seconds": 604800,
                    "reset_at": 1760600000
                }
            },
            "credits": {
                "has_credits": true,
                "unlimited": false,
                "balance": "42.5"
            }
        }));

        assert_eq!(data.len(), 3);
        assert_eq!(data[0].plan_name.as_deref(), Some("pro 5h"));
        assert_eq!(data[0].used, Some(81.5));
        assert_eq!(data[0].remaining, Some(18.5));
        assert_eq!(data[0].unit.as_deref(), Some("%"));

        assert_eq!(data[1].plan_name.as_deref(), Some("pro weekly"));
        assert_eq!(data[1].remaining, Some(88.0));

        assert_eq!(data[2].plan_name.as_deref(), Some("pro credits"));
        assert_eq!(data[2].remaining, Some(42.5));
        assert_eq!(data[2].unit.as_deref(), Some("USD"));
    }

    #[test]
    fn provider_uses_builtin_codex_usage_defaults_for_oauth_provider() {
        let provider = Provider::with_id(
            "oauth".to_string(),
            "OAuth".to_string(),
            json!({
                "oauth": {
                    "accountId": "acct-1",
                    "accessToken": "access",
                    "refreshToken": "refresh"
                },
                "auth": {
                    "tokens": {
                        "account_id": "acct-1",
                        "access_token": "access",
                        "refresh_token": "refresh"
                    }
                },
                "config": ""
            }),
            None,
        );

        assert!(provider_uses_builtin_codex_usage(
            AppType::Codex,
            &provider,
            None,
        ));
        assert!(!provider_uses_builtin_codex_usage(
            AppType::Codex,
            &provider,
            Some(&UsageScript {
                enabled: true,
                language: "javascript".to_string(),
                code: "return {}".to_string(),
                timeout: None,
                api_key: None,
                base_url: None,
                access_token: None,
                user_id: None,
                template_type: Some("general".to_string()),
                auto_query_interval: None,
            }),
        ));
    }
}
