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
    let base = user_home()?;
    Ok(base.join(".boxr"))
}

pub fn ensure_home() -> Result<PathBuf> {
    let home = boxr_home()?;
    create_private_dir(&home)?;
    Ok(home)
}

fn user_home() -> Result<PathBuf> {
    let keys: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME"]
    };
    for key in keys {
        if let Some(value) = env::var_os(key) {
            if !value.is_empty() {
                return Ok(PathBuf::from(value));
            }
        }
    }
    Err(anyhow!(
        "cannot locate the user home directory; set {HOME_ENV} to choose the boxr home"
    ))
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
