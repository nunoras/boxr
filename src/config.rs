use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub kinds: BTreeMap<String, String>,
    #[serde(default)]
    pub currency: Currency,
    #[serde(default)]
    pub prices: BTreeMap<String, Price>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub enum Currency {
    #[default]
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "EUR")]
    Eur,
}

impl Currency {
    pub fn code(&self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Eur => "EUR",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cached: f64,
    pub reasoning: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub account: Option<String>,
}

impl Config {
    pub fn load(home: &Path) -> Result<Config> {
        let path = Self::path(home);
        match fs::read_to_string(&path) {
            Ok(text) => {
                let config: Config = serde_json::from_str(&text)
                    .with_context(|| format!("reading config {}", path.display()))?;
                config.validate()?;
                Ok(config)
            }
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

    fn validate(&self) -> Result<()> {
        for (kind, description) in &self.kinds {
            if kind.trim().is_empty() || description.trim().is_empty() || description.contains('\n')
            {
                anyhow::bail!("each custom kind needs a name and one-line description");
            }
        }
        for (model, price) in &self.prices {
            if model.trim().is_empty() {
                anyhow::bail!("each price needs a model name");
            }
            for (token, rate) in [
                ("input", price.input),
                ("output", price.output),
                ("cached", price.cached),
                ("reasoning", price.reasoning),
            ] {
                if !rate.is_finite() || rate < 0.0 {
                    anyhow::bail!(
                        "the {token} price for {model} needs a number of {} per million tokens",
                        self.currency.code()
                    );
                }
            }
        }
        Ok(())
    }
}
