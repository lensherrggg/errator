use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const APP_DIR_NAME: &str = ".errator";

/// Resolve the Errator data directory. `ERRATOR_HOME` is an explicit override
/// used by portable installs and tests; otherwise we follow the PRD's
/// `~/.errator/` convention on every platform.
pub fn home_dir() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("ERRATOR_HOME")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    dirs::home_dir()
        .map(|home| home.join(APP_DIR_NAME))
        .context("无法确定用户主目录；请设置 ERRATOR_HOME")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(home_dir()?.join("config.toml"))
}

pub fn ignores_path() -> Result<PathBuf> {
    Ok(home_dir()?.join("ignores.json"))
}

pub fn models_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join("models"))
}

pub fn rulesets_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join("rulesets"))
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建目录 {}", parent.display()))?;
    }
    Ok(())
}

/// A stable, readable path for reports. Windows separators are normalized so
/// reports generated on different platforms compare cleanly.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = text.strip_prefix("./") {
        stripped.to_owned()
    } else {
        text
    }
}

/// Make a relative display path when possible, without requiring filesystem
/// canonicalization (which behaves inconsistently across Windows drives).
pub fn display_path_relative_to_cwd(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(relative) = path.strip_prefix(&cwd)
    {
        return display_path(relative);
    }
    display_path(path)
}

pub fn is_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "txt" | "md" | "docx"))
        .unwrap_or(false)
}
