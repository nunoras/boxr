use crate::fail::Fail;
use crate::harness::claude;
use crate::home::config_dir;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

struct BundledSkill {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

const BUNDLED: &[BundledSkill] = &[BundledSkill {
    name: "boxr-prompts",
    files: &[("SKILL.md", include_str!("../skills/boxr-prompts/SKILL.md"))],
}];

struct HarnessSpec {
    id: &'static str,
    config_env: &'static str,
    default_config_dir: &'static str,
}

const HARNESSES: &[HarnessSpec] = &[
    HarnessSpec {
        id: "claude",
        config_env: claude::CONFIG_DIR_ENV,
        default_config_dir: claude::DEFAULT_CONFIG_DIR,
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

pub fn names() -> impl Iterator<Item = &'static str> {
    BUNDLED.iter().map(|skill| skill.name)
}

pub fn install(harness: &str) -> Result<Vec<Installed>> {
    let mut installed = Vec::new();
    for spec in targets(harness)? {
        let skills = config_dir(spec.config_env, spec.default_config_dir)
            .ok_or_else(|| {
                anyhow!(
                    "cannot locate the user home directory; set {}",
                    spec.config_env
                )
            })?
            .join("skills");
        for skill in BUNDLED {
            let dir = skills.join(skill.name);
            write_skill(skill, &dir)?;
            installed.push(Installed {
                harness: spec.id,
                dir,
            });
        }
    }
    Ok(installed)
}

fn targets(harness: &str) -> Result<Vec<&'static HarnessSpec>, Fail> {
    if harness == "all" {
        return Ok(HARNESSES.iter().collect());
    }
    match HARNESSES.iter().find(|spec| spec.id == harness) {
        Some(spec) => Ok(vec![spec]),
        None => Err(Fail::usage(
            format!("unknown harness `{harness}`"),
            vec![format!(
                "Choose one of: {}, all",
                HARNESSES
                    .iter()
                    .map(|spec| spec.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            )],
        )),
    }
}

fn write_skill(skill: &BundledSkill, dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("replacing {}", dir.display()))?;
    }
    for (relative, contents) in skill.files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}
