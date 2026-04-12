use std::env;

const DEFAULT_APP_DISPLAY_NAME: &str = "TermPilot";
const DEFAULT_APP_CONFIG_DIR_NAME: &str = ".termpilot";
const DEFAULT_DEEPLINK_SCHEME: &str = "termpilot";
const DEFAULT_WEBDAV_REMOTE_ROOT: &str = "termpilot-sync";

fn resolve_identity_value(
    runtime_key: &str,
    compile_time_value: Option<&'static str>,
    default_value: &str,
) -> String {
    if let Ok(value) = env::var(runtime_key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    compile_time_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_value)
        .to_string()
}

pub fn app_display_name() -> String {
    resolve_identity_value(
        "CCSWITCH_APP_DISPLAY_NAME",
        option_env!("CCSWITCH_APP_DISPLAY_NAME"),
        DEFAULT_APP_DISPLAY_NAME,
    )
}

pub fn app_config_dir_name() -> String {
    resolve_identity_value(
        "CCSWITCH_APP_CONFIG_DIR_NAME",
        option_env!("CCSWITCH_APP_CONFIG_DIR_NAME"),
        DEFAULT_APP_CONFIG_DIR_NAME,
    )
}

pub fn deeplink_scheme() -> String {
    resolve_identity_value(
        "CCSWITCH_DEEPLINK_SCHEME",
        option_env!("CCSWITCH_DEEPLINK_SCHEME"),
        DEFAULT_DEEPLINK_SCHEME,
    )
}

pub fn webdav_remote_root() -> String {
    resolve_identity_value(
        "CCSWITCH_WEBDAV_REMOTE_ROOT",
        option_env!("CCSWITCH_WEBDAV_REMOTE_ROOT"),
        DEFAULT_WEBDAV_REMOTE_ROOT,
    )
}

#[allow(dead_code)]
pub fn uses_legacy_app_config_dir() -> bool {
    app_config_dir_name() == DEFAULT_APP_CONFIG_DIR_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn resolve_identity_value_falls_back_to_default() {
        std::env::remove_var("CCSWITCH_TEST_VALUE");

        assert_eq!(
            resolve_identity_value("CCSWITCH_TEST_VALUE", None, "fallback"),
            "fallback"
        );
    }

    #[test]
    #[serial_test::serial]
    fn resolve_identity_value_prefers_runtime_override() {
        std::env::set_var("CCSWITCH_TEST_VALUE", "runtime");

        assert_eq!(
            resolve_identity_value("CCSWITCH_TEST_VALUE", Some("compile"), "fallback"),
            "runtime"
        );

        std::env::remove_var("CCSWITCH_TEST_VALUE");
    }

    #[test]
    #[serial_test::serial]
    fn resolve_identity_value_uses_compile_time_value_when_runtime_missing() {
        std::env::remove_var("CCSWITCH_TEST_VALUE");

        assert_eq!(
            resolve_identity_value("CCSWITCH_TEST_VALUE", Some("compile"), "fallback"),
            "compile"
        );
    }
}
