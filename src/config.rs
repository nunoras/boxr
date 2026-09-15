use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Config {
    pub fn load(home: &Path) -> Result<Config> {
        let path = Self::path(home);
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("reading config {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(error) => {
                Err(anyhow::Error::from(error)
                    .context(format!("reading config {}", path.display())))
            }
        }
    }

    pub fn path(home: &Path) -> PathBuf {
        home.join(CONFIG_FILE)
    }
}
