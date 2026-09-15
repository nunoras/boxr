use crate::home::user_home;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

pub const SKILL_NAME: &str = "boxr-prompts";

const BUNDLED: &[(&str, &str)] = &[("SKILL.md", include_str!("../skills/boxr-prompts/SKILL.md"))];

struct HarnessSpec {
    id: &'static str,
    config_env: &'static str,
    default_config_dir: &'static str,
}

const HARNESSES: &[HarnessSpec] = &[
    HarnessSpec {
        id: "claude",
        config_env: "CLAUDE_CONFIG_DIR",
        default_config_dir: ".claude",
    },
    HarnessSpec {
        id: "codex",
        config_env: "CODEX_HOME",
        default_config_dir: ".codex",
    },
    HarnessSpec {
        id: "pi",
        config_env: "PI_CODING_AGENT_DIR",
        default_config_dir: ".pi/agent",
    },
];

pub struct Installed {
    pub harness: &'static str,
    pub dir: PathBuf,
}

pub fn ids() -> impl Iterator<Item = &'static str> {
    HARNESSES.iter().map(|spec| spec.id)
}

pub fn install(harness: &'static str) -> Result<Installed> {
    let spec = HARNESSES
        .iter()
        .find(|spec| spec.id == harness)
        .ok_or_else(|| anyhow!("unknown harness `{harness}`"))?;
    let dir = config_dir(spec)?.join("skills").join(SKILL_NAME);
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("replacing {}", dir.display()))?;
    }
    for (relative, contents) in BUNDLED {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(Installed { harness, dir })
}

fn config_dir(spec: &HarnessSpec) -> Result<PathBuf> {
    if let Some(value) = env::var_os(spec.config_env).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    user_home()
        .map(|home| home.join(spec.default_config_dir))
        .ok_or_else(|| {
            anyhow!(
                "cannot locate the user home directory; set {}",
                spec.config_env
            )
        })
}
