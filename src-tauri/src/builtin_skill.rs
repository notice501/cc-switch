use chrono::Utc;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::app_config::{AppType, InstalledSkill, SkillApps};
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::error::AppError;
use crate::services::skill::SkillService;

const DISPATCH_SKILL_ID: &str = "builtin:dispatch-task";
const DISPATCH_SKILL_NAME: &str = "dispatch-task";
const DISPATCH_SKILL_DIRECTORY: &str = "dispatch-task";
const DISPATCH_SKILL_DESCRIPTION: &str =
    "Run a subtask on a Claude or Codex provider configured in cc-switch, inspect dispatch status/history from Claude Code, and wait for the result in the current Claude Code session.";

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

pub fn ensure_dispatch_task_skill(db: &Arc<Database>) -> Result<InstalledSkill, AppError> {
    let installed_skills = db.get_all_installed_skills()?;
    if let Some(conflict) = installed_skills.values().find(|skill| {
        skill.directory.eq_ignore_ascii_case(DISPATCH_SKILL_DIRECTORY)
            && skill.id != DISPATCH_SKILL_ID
    }) {
        return Err(AppError::Message(format!(
            "Cannot install builtin dispatch skill because '{}' is already used by '{}' ({})",
            DISPATCH_SKILL_DIRECTORY, conflict.name, conflict.id
        )));
    }

    let ssot_dir = SkillService::get_ssot_dir().map_err(anyhow_to_app_error)?;
    let skill_dir = ssot_dir.join(DISPATCH_SKILL_DIRECTORY);
    write_dispatch_skill_files(&skill_dir)?;

    let installed_at = db
        .get_installed_skill(DISPATCH_SKILL_ID)?
        .map(|skill| skill.installed_at)
        .unwrap_or_else(|| Utc::now().timestamp());

    let skill = InstalledSkill {
        id: DISPATCH_SKILL_ID.to_string(),
        name: DISPATCH_SKILL_NAME.to_string(),
        description: Some(DISPATCH_SKILL_DESCRIPTION.to_string()),
        directory: DISPATCH_SKILL_DIRECTORY.to_string(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps::only(&AppType::Claude),
        installed_at,
    };

    db.save_skill(&skill)?;
    SkillService::sync_to_app_dir(DISPATCH_SKILL_DIRECTORY, &AppType::Claude)
        .map_err(anyhow_to_app_error)?;
    ensure_dispatch_status_line()?;

    Ok(skill)
}

fn write_dispatch_skill_files(skill_dir: &Path) -> Result<(), AppError> {
    fs::create_dir_all(skill_dir).map_err(|err| AppError::io(skill_dir, err))?;

    for relative_path in STALE_DISPATCH_SKILL_FILES {
        let stale_path = skill_dir.join(relative_path);
        if stale_path.exists() {
            fs::remove_file(&stale_path).map_err(|err| AppError::io(&stale_path, err))?;
        }
    }

    for (relative_path, contents) in DISPATCH_SKILL_FILES {
        let path = skill_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| AppError::io(parent, err))?;
        }

        let needs_write = match fs::read_to_string(&path) {
            Ok(existing) => existing != *contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => return Err(AppError::io(&path, err)),
        };

        if needs_write {
            fs::write(&path, contents).map_err(|err| AppError::io(&path, err))?;
        }
    }

    Ok(())
}

fn ensure_dispatch_status_line() -> Result<(), AppError> {
    let settings_path = get_claude_settings_path();
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

    let statusline_path = SkillService::get_app_skills_dir(&AppType::Claude)
        .map_err(anyhow_to_app_error)?
        .join(DISPATCH_SKILL_DIRECTORY)
        .join("scripts")
        .join("statusline.py");
    let command = format!("python3 {}", shell_quote(statusline_path.as_os_str().to_string_lossy().as_ref()));

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
