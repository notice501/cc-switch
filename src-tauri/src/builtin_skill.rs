use chrono::Utc;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::app_config::{AppType, InstalledSkill, SkillApps};
use crate::config::{
    get_app_config_dir, get_claude_settings_path, get_home_dir, read_json_file, write_json_file,
};
use crate::database::Database;
use crate::error::AppError;
use crate::services::skill::SkillService;

const DISPATCH_SKILL_ID: &str = "builtin:dispatch-task";
const DISPATCH_SKILL_NAME: &str = "dispatch-task";
const DISPATCH_SKILL_DIRECTORY: &str = "dispatch-task";
const DISPATCH_SKILL_DESCRIPTION: &str =
    "Legacy compatibility skill for running a child task on a Claude or Codex provider, inspecting runtime history, and optionally waiting in the current Claude Code session.";
const AGENT_SKILL_ID: &str = "builtin:agent";
const AGENT_SKILL_NAME: &str = "agent";
const AGENT_SKILL_DIRECTORY: &str = "agent";
const AGENT_SKILL_DESCRIPTION: &str =
    "Plan or run a child agent with suggested routing, terminal-first runtime modes, and Claude/Codex execution policies.";

const DISPATCH_SKILL_FILES: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../src/skills/task-dispatcher/SKILL.md"),
    ),
    (
        "scripts/dispatch.py",
        include_str!("../../src/skills/task-dispatcher/scripts/dispatch.py"),
    ),
    (
        "scripts/statusline.py",
        include_str!("../../src/skills/task-dispatcher/scripts/statusline.py"),
    ),
];

const AGENT_SKILL_FILES: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../src/skills/agent-workbench/SKILL.md"),
    ),
    (
        "scripts/agent.py",
        include_str!("../../src/skills/task-dispatcher/scripts/dispatch.py"),
    ),
    (
        "scripts/statusline.py",
        include_str!("../../src/skills/task-dispatcher/scripts/statusline.py"),
    ),
];

const STALE_DISPATCH_SKILL_FILES: &[&str] = &[
    "README.md",
    "QUICKSTART.md",
    "USAGE.md",
    "dispatch.py",
    "dispatch.sh",
    "main.py",
    "skill.json",
    "skill.toml",
];

const APP_CONFIG_DIR_PLACEHOLDER: &str = "__CCSWITCH_APP_CONFIG_DIR__";

pub fn ensure_dispatch_task_skill(db: &Arc<Database>) -> Result<InstalledSkill, AppError> {
    let dispatch_skill = ensure_builtin_skill(
        db,
        DISPATCH_SKILL_ID,
        DISPATCH_SKILL_NAME,
        DISPATCH_SKILL_DIRECTORY,
        DISPATCH_SKILL_DESCRIPTION,
        DISPATCH_SKILL_FILES,
        STALE_DISPATCH_SKILL_FILES,
    )?;
    let _agent_skill = ensure_builtin_skill(
        db,
        AGENT_SKILL_ID,
        AGENT_SKILL_NAME,
        AGENT_SKILL_DIRECTORY,
        AGENT_SKILL_DESCRIPTION,
        AGENT_SKILL_FILES,
        STALE_DISPATCH_SKILL_FILES,
    )?;
    ensure_agent_cli_launcher()?;
    ensure_dispatch_status_line()?;

    Ok(dispatch_skill)
}

fn ensure_builtin_skill(
    db: &Arc<Database>,
    skill_id: &str,
    skill_name: &str,
    skill_directory: &str,
    skill_description: &str,
    skill_files: &[(&str, &str)],
    stale_files: &[&str],
) -> Result<InstalledSkill, AppError> {
    let installed_skills = db.get_all_installed_skills()?;
    if let Some(conflict) = installed_skills.values().find(|skill| {
        skill.directory.eq_ignore_ascii_case(skill_directory) && skill.id != skill_id
    }) {
        return Err(AppError::Message(format!(
            "Cannot install builtin skill because '{}' is already used by '{}' ({})",
            skill_directory, conflict.name, conflict.id
        )));
    }

    let ssot_dir = SkillService::get_ssot_dir().map_err(anyhow_to_app_error)?;
    let skill_dir = ssot_dir.join(skill_directory);
    write_builtin_skill_files(&skill_dir, skill_files, stale_files)?;

    let installed_at = db
        .get_installed_skill(skill_id)?
        .map(|skill| skill.installed_at)
        .unwrap_or_else(|| Utc::now().timestamp());

    let skill = InstalledSkill {
        id: skill_id.to_string(),
        name: skill_name.to_string(),
        description: Some(skill_description.to_string()),
        directory: skill_directory.to_string(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps::only(&AppType::Claude),
        installed_at,
    };

    db.save_skill(&skill)?;
    SkillService::sync_to_app_dir(skill_directory, &AppType::Claude).map_err(anyhow_to_app_error)?;

    Ok(skill)
}

fn write_builtin_skill_files(
    skill_dir: &Path,
    skill_files: &[(&str, &str)],
    stale_files: &[&str],
) -> Result<(), AppError> {
    fs::create_dir_all(skill_dir).map_err(|err| AppError::io(skill_dir, err))?;
    let app_config_dir =
        crate::config::get_app_config_dir().to_string_lossy().replace('\\', "\\\\");

    for relative_path in stale_files {
        let stale_path = skill_dir.join(relative_path);
        if stale_path.exists() {
            fs::remove_file(&stale_path).map_err(|err| AppError::io(&stale_path, err))?;
        }
    }

    for (relative_path, contents) in skill_files {
        let path = skill_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| AppError::io(parent, err))?;
        }

        let rendered_contents = contents.replace(APP_CONFIG_DIR_PLACEHOLDER, &app_config_dir);
        let needs_write = match fs::read_to_string(&path) {
            Ok(existing) => existing != rendered_contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => return Err(AppError::io(&path, err)),
        };

        if needs_write {
            fs::write(&path, rendered_contents).map_err(|err| AppError::io(&path, err))?;
        }
    }

    Ok(())
}

fn ensure_agent_cli_launcher() -> Result<(), AppError> {
    let cli_dir = get_app_config_dir().join("cli");
    fs::create_dir_all(&cli_dir).map_err(|err| AppError::io(&cli_dir, err))?;

    let app_config_dir = get_app_config_dir().to_string_lossy().replace('\\', "\\\\");
    let engine_path = cli_dir.join("agent.py");
    let rendered = include_str!("../../src/skills/task-dispatcher/scripts/dispatch.py")
        .replace(APP_CONFIG_DIR_PLACEHOLDER, &app_config_dir);
    fs::write(&engine_path, rendered).map_err(|err| AppError::io(&engine_path, err))?;

    let launcher_dir = get_home_dir().join(".local").join("bin");
    fs::create_dir_all(&launcher_dir).map_err(|err| AppError::io(&launcher_dir, err))?;
    let launcher_path = launcher_dir.join("agent");
    let launcher = format!(
        "#!/bin/sh\nexport CCSWITCH_AGENT_ENTRYPOINT=agent\nexec python3 '{}' \"$@\"\n",
        engine_path.display()
    );
    fs::write(&launcher_path, launcher).map_err(|err| AppError::io(&launcher_path, err))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&launcher_path)
            .map_err(|err| AppError::io(&launcher_path, err))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&launcher_path, permissions)
            .map_err(|err| AppError::io(&launcher_path, err))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        test_home: Option<std::ffi::OsString>,
        app_config_dir_name: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(test_home: &str, app_config_dir_name: &str) -> Self {
            let guard = Self {
                test_home: std::env::var_os("CC_SWITCH_TEST_HOME"),
                app_config_dir_name: std::env::var_os("CCSWITCH_APP_CONFIG_DIR_NAME"),
            };
            std::env::set_var("CC_SWITCH_TEST_HOME", test_home);
            std::env::set_var("CCSWITCH_APP_CONFIG_DIR_NAME", app_config_dir_name);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            match &self.app_config_dir_name {
                Some(value) => std::env::set_var("CCSWITCH_APP_CONFIG_DIR_NAME", value),
                None => std::env::remove_var("CCSWITCH_APP_CONFIG_DIR_NAME"),
            }
        }
    }

    #[test]
    #[serial]
    fn dispatch_skill_files_use_instance_config_dir() {
        let _guard = EnvGuard::set("/tmp/ccswitch-skill-home", ".ccswitch-pro");

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        write_builtin_skill_files(
            temp_dir.path(),
            DISPATCH_SKILL_FILES,
            STALE_DISPATCH_SKILL_FILES,
        )
        .expect("write dispatch skill");

        let dispatch = std::fs::read_to_string(temp_dir.path().join("scripts/dispatch.py"))
            .expect("read dispatch.py");
        let statusline = std::fs::read_to_string(temp_dir.path().join("scripts/statusline.py"))
            .expect("read statusline.py");

        assert!(dispatch.contains("/tmp/ccswitch-skill-home/.ccswitch-pro"));
        assert!(statusline.contains("/tmp/ccswitch-skill-home/.ccswitch-pro"));
    }
}

fn ensure_dispatch_status_line() -> Result<(), AppError> {
    let statusline_path = SkillService::get_app_skills_dir(&AppType::Claude)
        .map_err(anyhow_to_app_error)?
        .join(DISPATCH_SKILL_DIRECTORY)
        .join("scripts")
        .join("statusline.py");
    let command = format!(
        "python3 {}",
        shell_quote(statusline_path.as_os_str().to_string_lossy().as_ref())
    );

    for settings_path in collect_claude_settings_paths()? {
        ensure_dispatch_status_line_at(&settings_path, &command)?;
    }

    Ok(())
}

fn collect_claude_settings_paths() -> Result<Vec<std::path::PathBuf>, AppError> {
    let mut paths = vec![get_claude_settings_path()];
    let alias_homes_dir = get_app_config_dir().join("alias-homes");

    if alias_homes_dir.exists() {
        let entries = fs::read_dir(&alias_homes_dir).map_err(|err| AppError::io(&alias_homes_dir, err))?;
        for entry in entries.flatten() {
            let alias_claude_dir = entry.path().join(".claude");
            if !alias_claude_dir.exists() {
                continue;
            }
            let settings_path = alias_claude_dir.join("settings.json");
            if !paths.iter().any(|existing| existing == &settings_path) {
                paths.push(settings_path);
            }
        }
    }

    Ok(paths)
}

fn ensure_dispatch_status_line_at(settings_path: &Path, command: &str) -> Result<(), AppError> {
    let mut settings: Value = if settings_path.exists() {
        read_json_file(&settings_path)?
    } else {
        json!({})
    };

    let Some(root) = settings.as_object_mut() else {
        return Err(AppError::Message(format!(
            "Claude settings root must be a JSON object: {}",
            settings_path.display()
        )));
    };

    root.insert(
        "statusLine".to_string(),
        json!({
            "type": "command",
            "command": command,
            "padding": 0,
        }),
    );

    write_json_file(&settings_path, &settings)?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let escaped = value.replace('\'', r"'\''");
    format!("'{escaped}'")
}

fn anyhow_to_app_error(err: anyhow::Error) -> AppError {
    AppError::Message(err.to_string())
}
