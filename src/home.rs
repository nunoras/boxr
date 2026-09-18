use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

pub const HOME_ENV: &str = "BOXR_HOME";

pub fn boxr_home() -> Result<PathBuf> {
    if let Some(value) = env::var_os(HOME_ENV) {
        if !value.is_empty() {
            return Ok(PathBuf::from(value));
        }
    }
    let base = user_home().ok_or_else(|| {
        anyhow!("cannot locate the user home directory; set {HOME_ENV} to choose the boxr home")
    })?;
    Ok(base.join(".boxr"))
}

pub fn ensure_home() -> Result<PathBuf> {
    let home = boxr_home()?;
    create_private_dir(&home)?;
    Ok(home)
}

pub fn user_home() -> Option<PathBuf> {
    let keys: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME"]
    };
    keys.iter()
        .filter_map(env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn config_dir(override_env: &str, default_dir: &str) -> Option<PathBuf> {
    match env::var_os(override_env) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => user_home().map(|home| home.join(default_dir)),
    }
}

pub fn create_private_dir(path: &std::path::Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("creating directory {}", path.display()))?;
    restrict_dir(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restricting permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_dir(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn restrict_file(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting permissions on {}", path.display()))
}

#[cfg(not(unix))]
pub fn restrict_file(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

pub fn write_file_atomically(path: &std::path::Path, body: &str) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, body).with_context(|| format!("writing {}", temp.display()))?;
    restrict_file(&temp)?;
    fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}
