use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::json;

use cc_switch_lib::{
    get_claude_settings_path, get_codex_auth_path, read_json_file, write_codex_live_atomic,
    AppError, AppType, McpApps, McpServer, MultiAppConfig, Provider, ProviderMeta,
    ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{
    create_test_state, create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex,
};

fn sanitize_provider_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

fn make_test_jwt(payload: serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&payload).expect("serialize jwt payload"),
    );
    format!("{header}.{payload}.sig")
}

#[test]
fn migrate_legacy_common_config_usage_marks_historical_provider_enabled() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "legacy-provider".to_string();
        manager.providers.insert(
            "legacy-provider".to_string(),
            Provider::with_id(
                "legacy-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "includeCoAuthoredBy": false,
                    "env": {
                        "ANTHROPIC_API_KEY": "legacy-key"
                    }
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{ "includeCoAuthoredBy": false }"#.to_string()),
        )
        .expect("set common config snippet");

    ProviderService::migrate_legacy_common_config_usage_if_needed(&state, AppType::Claude)
        .expect("migrate legacy common config");

    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get providers after migration");
    let provider = providers
        .get("legacy-provider")
        .expect("legacy provider exists");

    assert_eq!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.common_config_enabled),
        Some(true),
        "historical provider should be explicitly marked as using common config"
    );
    assert!(
        provider
            .settings_config
            .get("includeCoAuthoredBy")
            .is_none(),
        "common config fields should be stripped from provider storage after migration"
    );
    assert_eq!(
        provider
            .settings_config
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str()),
        Some("legacy-key"),
        "provider-specific auth should remain untouched"
    );
}

#[test]
fn provider_service_switch_codex_updates_live_and_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let legacy_auth = json!({ "OPENAI_API_KEY": "legacy-key" });
    let legacy_config = r#"[mcp_servers.legacy]
type = "stdio"
command = "echo"
"#;
    write_codex_live_atomic(&legacy_auth, Some(legacy_config))
        .expect("seed existing codex live config");

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "stale"},
                    "config": "stale-config"
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Latest".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "fresh-key"},
                    "config": r#"[mcp_servers.latest]
type = "stdio"
command = "say"
"#
                }),
                None,
            ),
        );
    }

    // 使用新的统一 MCP 结构（v3.7.0+）
    let servers = initial_config
        .mcp
        .servers
        .get_or_insert_with(Default::default);
    servers.insert(
        "echo-server".into(),
        McpServer {
            id: "echo-server".into(),
            name: "Echo Server".into(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: false,
                codex: true,
                gemini: false,
                opencode: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    );

    let state = create_test_state_with_config(&initial_config).expect("create test state");

    ProviderService::switch(&state, AppType::Codex, "new-provider")
        .expect("switch provider should succeed");

    let auth_value: serde_json::Value =
        read_json_file(&cc_switch_lib::get_codex_auth_path()).expect("read auth.json");
    assert_eq!(
        auth_value.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
        Some("fresh-key"),
        "live auth.json should reflect new provider"
    );

    let config_text =
        std::fs::read_to_string(cc_switch_lib::get_codex_config_path()).expect("read config.toml");
    assert!(
        config_text.contains("mcp_servers.echo-server"),
        "config.toml should contain synced MCP servers"
    );

    let current_id = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("read current provider after switch");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read providers after switch");

    let new_provider = providers.get("new-provider").expect("new provider exists");
    let new_config_text = new_provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // provider 存储的是原始配置，不包含 MCP 同步后的内容
    assert!(
        new_config_text.contains("mcp_servers.latest"),
        "provider config should contain original MCP servers"
    );
    // live 文件额外包含同步的 MCP 服务器
    assert!(
        config_text.contains("mcp_servers.echo-server"),
        "live config should include synced MCP servers"
    );

    let legacy = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    let legacy_auth_value = legacy
        .settings_config
        .get("auth")
        .and_then(|v| v.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        legacy_auth_value, "legacy-key",
        "previous provider should be backfilled with live auth"
    );
}

#[test]
fn sync_current_provider_for_app_keeps_live_takeover_and_updates_restore_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "current-provider".to_string();

        let mut provider = Provider::with_id(
            "current-provider".to_string(),
            "Current".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "real-token",
                    "ANTHROPIC_BASE_URL": "https://claude.example"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        manager
            .providers
            .insert("current-provider".to_string(), provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{ "includeCoAuthoredBy": false }"#.to_string()),
        )
        .expect("set common config snippet");

    let taken_over_live = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:5000",
            "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED"
        }
    });
    let settings_path = get_claude_settings_path();
    std::fs::create_dir_all(settings_path.parent().expect("settings dir")).expect("create dir");
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&taken_over_live).expect("serialize taken over live"),
    )
    .expect("write taken over live");

    futures::executor::block_on(state.db.save_live_backup("claude", "{\"env\":{}}"))
        .expect("seed live backup");

    let mut proxy_config = futures::executor::block_on(state.db.get_proxy_config_for_app("claude"))
        .expect("get proxy config");
    proxy_config.enabled = true;
    futures::executor::block_on(state.db.update_proxy_config_for_app(proxy_config))
        .expect("enable takeover");

    ProviderService::sync_current_provider_for_app(&state, AppType::Claude)
        .expect("sync current provider should succeed");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read live settings after sync");
    assert_eq!(
        live_after, taken_over_live,
        "sync should not overwrite live config while takeover is active"
    );

    let backup = futures::executor::block_on(state.db.get_live_backup("claude"))
        .expect("get live backup")
        .expect("backup exists");
    let backup_value: serde_json::Value =
        serde_json::from_str(&backup.original_config).expect("parse backup value");

    assert_eq!(
        backup_value
            .get("includeCoAuthoredBy")
            .and_then(|v| v.as_bool()),
        Some(false),
        "restore backup should receive the updated effective config"
    );
    assert_eq!(
        backup_value
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|v| v.as_str()),
        Some("real-token"),
        "restore backup should preserve the provider token rather than proxy placeholder"
    );
}

#[test]
fn explicitly_cleared_common_snippet_is_not_auto_extracted() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");
    state
        .db
        .set_config_snippet_cleared(AppType::Claude.as_str(), true)
        .expect("mark snippet explicitly cleared");

    assert!(
        !state
            .db
            .should_auto_extract_config_snippet(AppType::Claude.as_str())
            .expect("check auto-extract eligibility"),
        "explicitly cleared snippets should block auto-extraction"
    );

    state
        .db
        .set_config_snippet(AppType::Claude.as_str(), Some("{}".to_string()))
        .expect("set snippet");
    state
        .db
        .set_config_snippet_cleared(AppType::Claude.as_str(), false)
        .expect("clear explicit-empty marker");

    assert!(
        !state
            .db
            .should_auto_extract_config_snippet(AppType::Claude.as_str())
            .expect("check auto-extract after snippet saved"),
        "existing snippets should also block auto-extraction"
    );
}

#[test]
fn legacy_common_config_migration_flag_roundtrip() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    assert!(
        !state
            .db
            .is_legacy_common_config_migrated()
            .expect("initial migration flag"),
        "migration flag should default to false"
    );

    state
        .db
        .set_legacy_common_config_migrated(true)
        .expect("set migration flag");
    assert!(
        state
            .db
            .is_legacy_common_config_migrated()
            .expect("read migration flag"),
        "migration flag should persist once set"
    );

    state
        .db
        .set_legacy_common_config_migrated(false)
        .expect("clear migration flag");
    assert!(
        !state
            .db
            .is_legacy_common_config_migrated()
            .expect("read migration flag after clear"),
        "migration flag should be removable for tests/debugging"
    );
}

#[test]
fn switch_packycode_gemini_updates_security_selected_type() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "packy-gemini".to_string();
        manager.providers.insert(
            "packy-gemini".to_string(),
            Provider::with_id(
                "packy-gemini".to_string(),
                "PackyCode".to_string(),
                json!({
                    "env": {
                        "GEMINI_API_KEY": "pk-key",
                        "GOOGLE_GEMINI_BASE_URL": "https://www.packyapi.com"
                    }
                }),
                Some("https://www.packyapi.com".to_string()),
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "packy-gemini")
        .expect("switching to PackyCode Gemini should succeed");

    // Gemini security settings are written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let settings_path = home.join(".gemini").join("settings.json");
    assert!(
        settings_path.exists(),
        "Gemini settings.json should exist at {}",
        settings_path.display()
    );
    let raw = std::fs::read_to_string(&settings_path).expect("read gemini settings.json");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("parse gemini settings.json after switch");

    assert_eq!(
        value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "PackyCode Gemini should set security.auth.selectedType"
    );
}

#[test]
fn packycode_partner_meta_triggers_security_flag_even_without_keywords() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "packy-meta".to_string();
        let mut provider = Provider::with_id(
            "packy-meta".to_string(),
            "Generic Gemini".to_string(),
            json!({
                "env": {
                    "GEMINI_API_KEY": "pk-meta",
                    "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com"
                }
            }),
            Some("https://example.com".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            partner_promotion_key: Some("packycode".to_string()),
            ..ProviderMeta::default()
        });
        manager.providers.insert("packy-meta".to_string(), provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "packy-meta")
        .expect("switching to partner meta provider should succeed");

    // Gemini security settings are written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let settings_path = home.join(".gemini").join("settings.json");
    assert!(
        settings_path.exists(),
        "Gemini settings.json should exist at {}",
        settings_path.display()
    );
    let raw = std::fs::read_to_string(&settings_path).expect("read gemini settings.json");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("parse gemini settings.json after switch");

    assert_eq!(
        value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "Partner meta should set security.auth.selectedType even without packy keywords"
    );
}

#[test]
fn switch_google_official_gemini_sets_oauth_security() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "google-official".to_string();
        let mut provider = Provider::with_id(
            "google-official".to_string(),
            "Google".to_string(),
            json!({
                "env": {}
            }),
            Some("https://ai.google.dev".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            partner_promotion_key: Some("google-official".to_string()),
            ..ProviderMeta::default()
        });
        manager
            .providers
            .insert("google-official".to_string(), provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "google-official")
        .expect("switching to Google official Gemini should succeed");

    // Gemini security settings are written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let gemini_settings = home.join(".gemini").join("settings.json");
    assert!(
        gemini_settings.exists(),
        "Gemini settings.json should exist at {}",
        gemini_settings.display()
    );
    let gemini_raw = std::fs::read_to_string(&gemini_settings).expect("read gemini settings");
    let gemini_value: serde_json::Value =
        serde_json::from_str(&gemini_raw).expect("parse gemini settings");

    assert_eq!(
        gemini_value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("oauth-personal"),
        "Gemini settings json should reflect oauth-personal for Google Official"
    );
}

#[test]
fn provider_service_switch_claude_updates_live_and_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let legacy_live = json!({
        "env": {
            "ANTHROPIC_API_KEY": "legacy-key"
        },
        "workspace": {
            "path": "/tmp/workspace"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&legacy_live).expect("serialize legacy live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "stale-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Fresh Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "fresh-key" },
                    "workspace": { "path": "/tmp/new-workspace" }
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Claude, "new-provider")
        .expect("switch provider should succeed");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read claude live settings");
    assert_eq!(
        live_after
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|key| key.as_str()),
        Some("fresh-key"),
        "live settings.json should reflect new provider auth"
    );

    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");
    let current_id = state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let legacy_provider = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    assert_eq!(
        legacy_provider.settings_config, legacy_live,
        "previous provider should receive backfilled live config"
    );
}

#[test]
fn provider_service_switch_missing_provider_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let err = ProviderService::switch(&state, AppType::Claude, "missing")
        .expect_err("switching missing provider should fail");
    match err {
        AppError::Message(msg) => {
            assert!(
                msg.contains("不存在") || msg.contains("not found"),
                "expected provider not found message, got {msg}"
            );
        }
        other => panic!("expected Message error for provider not found, got {other:?}"),
    }
}

#[test]
fn provider_service_switch_codex_missing_auth_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.providers.insert(
            "invalid".to_string(),
            Provider::with_id(
                "invalid".to_string(),
                "Broken Codex".to_string(),
                json!({
                    "config": "[mcp_servers.test]\ncommand = \"noop\""
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    let err = ProviderService::switch(&state, AppType::Codex, "invalid")
        .expect_err("switching should fail without auth");
    match err {
        AppError::Config(msg) => assert!(
            msg.contains("auth"),
            "expected auth related message, got {msg}"
        ),
        other => panic!("expected config error, got {other:?}"),
    }
}

#[test]
fn provider_service_delete_codex_removes_provider_and_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "keep".to_string();
        manager.providers.insert(
            "keep".to_string(),
            Provider::with_id(
                "keep".to_string(),
                "Keep".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "keep-key"},
                    "config": ""
                }),
                None,
            ),
        );
        manager.providers.insert(
            "to-delete".to_string(),
            Provider::with_id(
                "to-delete".to_string(),
                "DeleteCodex".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "delete-key"},
                    "config": ""
                }),
                None,
            ),
        );
    }

    let sanitized = sanitize_provider_name("DeleteCodex");
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    let auth_path = codex_dir.join(format!("auth-{sanitized}.json"));
    let cfg_path = codex_dir.join(format!("config-{sanitized}.toml"));
    std::fs::write(&auth_path, "{}").expect("seed auth file");
    std::fs::write(&cfg_path, "base_url = \"https://example\"").expect("seed config file");

    let app_state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::delete(&app_state, AppType::Codex, "to-delete")
        .expect("delete provider should succeed");

    let providers = app_state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get all providers");
    assert!(
        !providers.contains_key("to-delete"),
        "provider entry should be removed"
    );
    // v3.7.0+ 不再使用供应商特定文件（如 auth-*.json, config-*.toml）
    // 删除供应商只影响数据库记录，不清理这些旧格式文件
}

#[test]
fn provider_service_delete_claude_removes_provider_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "keep".to_string();
        manager.providers.insert(
            "keep".to_string(),
            Provider::with_id(
                "keep".to_string(),
                "Keep".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "keep-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "delete".to_string(),
            Provider::with_id(
                "delete".to_string(),
                "DeleteClaude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "delete-key" }
                }),
                None,
            ),
        );
    }

    let sanitized = sanitize_provider_name("DeleteClaude");
    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).expect("create claude dir");
    let by_name = claude_dir.join(format!("settings-{sanitized}.json"));
    let by_id = claude_dir.join("settings-delete.json");
    std::fs::write(&by_name, "{}").expect("seed settings by name");
    std::fs::write(&by_id, "{}").expect("seed settings by id");

    let app_state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::delete(&app_state, AppType::Claude, "delete").expect("delete claude provider");

    let providers = app_state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");
    assert!(
        !providers.contains_key("delete"),
        "claude provider should be removed"
    );
    // v3.7.0+ 不再使用供应商特定文件（如 settings-*.json）
    // 删除供应商只影响数据库记录，不清理这些旧格式文件
}

#[test]
fn provider_service_delete_current_provider_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "keep".to_string();
        manager.providers.insert(
            "keep".to_string(),
            Provider::with_id(
                "keep".to_string(),
                "Keep".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "keep-key" }
                }),
                None,
            ),
        );
    }

    let app_state = create_test_state_with_config(&config).expect("create test state");

    let err = ProviderService::delete(&app_state, AppType::Claude, "keep")
        .expect_err("deleting current provider should fail");
    match err {
        AppError::Localized { zh, .. } => assert!(
            zh.contains("不能删除当前正在使用的供应商")
                || zh.contains("无法删除当前正在使用的供应商"),
            "unexpected message: {zh}"
        ),
        AppError::Config(msg) => assert!(
            msg.contains("不能删除当前正在使用的供应商")
                || msg.contains("无法删除当前正在使用的供应商"),
            "unexpected message: {msg}"
        ),
        AppError::Message(msg) => assert!(
            msg.contains("不能删除当前正在使用的供应商")
                || msg.contains("无法删除当前正在使用的供应商"),
            "unexpected message: {msg}"
        ),
        other => panic!("expected Config/Message error, got {other:?}"),
    }
}

#[test]
fn provider_service_add_codex_rejects_duplicate_oauth_account() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let future_exp = chrono::Utc::now().timestamp() + 3600;

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "primary".to_string();
        manager.providers.insert(
            "primary".to_string(),
            Provider::with_id(
                "primary".to_string(),
                "Primary Codex".to_string(),
                json!({
                    "auth": {
                        "auth_mode": "chatgpt",
                        "OPENAI_API_KEY": null,
                        "tokens": {
                            "account_id": "acct-1",
                            "access_token": "at-1",
                            "refresh_token": "rt-1"
                        }
                    },
                    "oauth": {
                        "authMode": "chatgpt",
                        "accountId": "acct-1",
                        "accessToken": "at-1",
                        "refreshToken": "rt-1",
                        "accessTokenExpiresAt": future_exp
                    },
                    "config": "model = \"gpt-5.4\""
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    let duplicate = Provider::with_id(
        "secondary".to_string(),
        "Secondary Codex".to_string(),
        json!({
            "auth": {
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": {
                    "account_id": "acct-1",
                    "access_token": "at-2",
                    "refresh_token": "rt-2"
                }
            },
            "oauth": {
                "authMode": "chatgpt",
                "accountId": "acct-1",
                "accessToken": "at-2",
                "refreshToken": "rt-2",
                "accessTokenExpiresAt": future_exp
            },
            "config": "model = \"gpt-5.4\""
        }),
        None,
    );

    let err = ProviderService::add(&state, AppType::Codex, duplicate)
        .expect_err("duplicate oauth account should be rejected");
    assert!(
        err.to_string().contains("Primary Codex"),
        "expected duplicate account error to mention existing provider, got {err}"
    );
}

#[test]
fn provider_service_switch_codex_oauth_writes_chatgpt_auth_json() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let future_exp = chrono::Utc::now().timestamp() + 3600;

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "manual".to_string();
        manager.providers.insert(
            "manual".to_string(),
            Provider::with_id(
                "manual".to_string(),
                "Manual".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "manual-key"},
                    "config": "model = \"gpt-5.4\""
                }),
                None,
            ),
        );
        manager.providers.insert(
            "oauth".to_string(),
            Provider::with_id(
                "oauth".to_string(),
                "OAuth".to_string(),
                json!({
                    "auth": {
                        "auth_mode": "chatgpt",
                        "OPENAI_API_KEY": null,
                        "tokens": {
                            "id_token": "id-1",
                            "account_id": "acct-oauth",
                            "access_token": "access-oauth",
                            "refresh_token": "refresh-oauth"
                        },
                        "last_refresh": "2026-03-28T00:00:00Z"
                    },
                    "oauth": {
                        "authMode": "chatgpt",
                        "accountId": "acct-oauth",
                        "accessToken": "access-oauth",
                        "refreshToken": "refresh-oauth",
                        "idToken": "id-1",
                        "accessTokenExpiresAt": future_exp
                    },
                    "config": "model = \"gpt-5.4\""
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Codex, "oauth")
        .expect("switch to oauth provider should succeed");

    let auth_value: serde_json::Value =
        read_json_file(&get_codex_auth_path()).expect("read oauth auth.json");
    assert_eq!(
        auth_value.get("auth_mode").and_then(|value| value.as_str()),
        Some("chatgpt")
    );
    assert_eq!(
        auth_value
            .get("tokens")
            .and_then(|tokens| tokens.get("account_id"))
            .and_then(|value| value.as_str()),
        Some("acct-oauth")
    );
}

#[test]
fn provider_service_switch_codex_oauth_backfills_live_auth_without_dropping_oauth() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let future_exp = chrono::Utc::now().timestamp() + 3600;

    let access_a_stored = make_test_jwt(json!({
        "exp": future_exp - 120,
        "email": "a@example.com",
        "name": "Account A",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acct-a",
            "chatgpt_plan_type": "plus",
            "chatgpt_user_id": "user-a"
        }
    }));
    let access_a_live = make_test_jwt(json!({
        "exp": future_exp,
        "email": "a@example.com",
        "name": "Account A Live",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acct-a",
            "chatgpt_plan_type": "pro",
            "chatgpt_user_id": "user-a"
        }
    }));
    let id_a_stored = make_test_jwt(json!({
        "exp": future_exp - 120,
        "email": "a@example.com",
        "name": "Account A",
        "auth_provider": "google"
    }));
    let id_a_live = make_test_jwt(json!({
        "exp": future_exp,
        "email": "a@example.com",
        "name": "Account A Live",
        "auth_provider": "google"
    }));

    let access_b = make_test_jwt(json!({
        "exp": future_exp,
        "email": "b@example.com",
        "name": "Account B",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acct-b",
            "chatgpt_plan_type": "plus",
            "chatgpt_user_id": "user-b"
        }
    }));
    let id_b = make_test_jwt(json!({
        "exp": future_exp,
        "email": "b@example.com",
        "name": "Account B",
        "auth_provider": "google"
    }));

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "oauth-a".to_string();
        manager.providers.insert(
            "oauth-a".to_string(),
            Provider::with_id(
                "oauth-a".to_string(),
                "OAuth A".to_string(),
                json!({
                    "auth": {
                        "auth_mode": "chatgpt",
                        "OPENAI_API_KEY": null,
                        "tokens": {
                            "id_token": id_a_stored,
                            "account_id": "acct-a",
                            "access_token": access_a_stored,
                            "refresh_token": "refresh-a-stored"
                        },
                        "last_refresh": "2026-03-28T00:00:00Z"
                    },
                    "oauth": {
                        "authMode": "chatgpt",
                        "accountId": "acct-a",
                        "accessToken": access_a_stored,
                        "refreshToken": "refresh-a-stored",
                        "idToken": id_a_stored,
                        "accessTokenExpiresAt": future_exp - 120,
                        "idTokenExpiresAt": future_exp - 120,
                        "email": "a@example.com",
                        "name": "Account A",
                        "authProvider": "google",
                        "planType": "plus",
                        "chatgptUserId": "user-a",
                        "lastRefresh": "2026-03-28T00:00:00Z"
                    },
                    "config": "model = \"provider-a\"\n"
                }),
                None,
            ),
        );
        manager.providers.insert(
            "oauth-b".to_string(),
            Provider::with_id(
                "oauth-b".to_string(),
                "OAuth B".to_string(),
                json!({
                    "auth": {
                        "auth_mode": "chatgpt",
                        "OPENAI_API_KEY": null,
                        "tokens": {
                            "id_token": id_b,
                            "account_id": "acct-b",
                            "access_token": access_b,
                            "refresh_token": "refresh-b"
                        },
                        "last_refresh": "2026-03-29T00:00:00Z"
                    },
                    "oauth": {
                        "authMode": "chatgpt",
                        "accountId": "acct-b",
                        "accessToken": access_b,
                        "refreshToken": "refresh-b",
                        "idToken": id_b,
                        "accessTokenExpiresAt": future_exp,
                        "idTokenExpiresAt": future_exp,
                        "email": "b@example.com",
                        "name": "Account B",
                        "authProvider": "google",
                        "planType": "plus",
                        "chatgptUserId": "user-b",
                        "lastRefresh": "2026-03-29T00:00:00Z"
                    },
                    "config": "model = \"provider-b\"\n"
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    write_codex_live_atomic(
        &json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": id_a_live,
                "account_id": "acct-a",
                "access_token": access_a_live,
                "refresh_token": "refresh-a-live"
            },
            "last_refresh": "2026-04-12T00:00:00Z"
        }),
        Some("model = \"live-current\"\n"),
    )
    .expect("write live codex state");

    ProviderService::switch(&state, AppType::Codex, "oauth-b")
        .expect("switch to oauth-b should succeed");

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read codex providers after switch");
    let provider_a = providers.get("oauth-a").expect("oauth-a still exists");

    assert_eq!(
        provider_a
            .settings_config
            .pointer("/auth/tokens/access_token")
            .and_then(|value| value.as_str()),
        Some(access_a_live.as_str()),
        "current live access token should be backfilled into provider auth snapshot"
    );
    assert_eq!(
        provider_a
            .settings_config
            .pointer("/oauth/accessToken")
            .and_then(|value| value.as_str()),
        Some(access_a_live.as_str()),
        "oauth metadata should keep the refreshed live access token"
    );
    assert_eq!(
        provider_a
            .settings_config
            .pointer("/oauth/refreshToken")
            .and_then(|value| value.as_str()),
        Some("refresh-a-live"),
        "oauth metadata should keep the refreshed live refresh token"
    );
    assert_eq!(
        provider_a
            .settings_config
            .pointer("/oauth/planType")
            .and_then(|value| value.as_str()),
        Some("pro"),
        "oauth metadata should be rebuilt from the live auth snapshot"
    );
    assert_eq!(
        provider_a
            .settings_config
            .get("config")
            .and_then(|value| value.as_str()),
        Some("model = \"provider-a\"\n"),
        "oauth account switch should not overwrite provider-local config from live config.toml"
    );

    let auth_value: serde_json::Value =
        read_json_file(&get_codex_auth_path()).expect("read switched auth.json");
    assert_eq!(
        auth_value
            .get("tokens")
            .and_then(|tokens| tokens.get("account_id"))
            .and_then(|value| value.as_str()),
        Some("acct-b"),
        "target oauth account should become active in live auth.json"
    );
}
