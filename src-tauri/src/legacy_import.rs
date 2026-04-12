use std::fs;
use std::path::{Path, PathBuf};

use crate::config::get_home_dir;
use crate::error::AppError;

pub fn bootstrap_legacy_app_dir() -> Result<(), AppError> {
    let home = get_home_dir();
    let target_dir = home.join(crate::app_identity::app_config_dir_name());
    if has_runtime_state(&target_dir) {
        return Ok(());
    }

    let candidates = legacy_candidates(&home);
    let Some(source_dir) = candidates.into_iter().find(|path| has_runtime_state(path)) else {
        return Ok(());
    };

    log::info!(
        "Bootstrapping legacy runtime state from {} into {}",
        source_dir.display(),
        target_dir.display()
    );
    copy_dir_recursive(&source_dir, &target_dir)?;
    Ok(())
}

fn legacy_candidates(home: &Path) -> Vec<PathBuf> {
    match crate::app_identity::app_config_dir_name().as_str() {
        ".termpilot" => vec![home.join(".cc-switch"), home.join(".ccswitch-pro")],
        ".termpilot-studio" => vec![home.join(".ccswitch-pro"), home.join(".cc-switch")],
        _ => vec![home.join(".ccswitch-pro"), home.join(".cc-switch")],
    }
}

fn has_runtime_state(dir: &Path) -> bool {
    dir.join("cc-switch.db").exists()
        || dir.join("config.json").exists()
        || dir.join("dispatch-history.jsonl").exists()
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|err| AppError::io(target, err))?;

    let entries = fs::read_dir(source).map_err(|err| AppError::io(source, err))?;
    for entry in entries {
        let entry = entry.map_err(|err| AppError::io(source, err))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|err| AppError::io(&source_path, err))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if !target_path.exists() {
            fs::copy(&source_path, &target_path)
                .map_err(|err| AppError::io(&target_path, err))?;
        }
    }

    Ok(())
}
