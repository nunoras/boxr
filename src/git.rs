use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub repo: PathBuf,
    pub base: Option<String>,
    pub commits: Vec<String>,
    pub files: Vec<String>,
    pub reverted: Option<Vec<String>>,
}

pub fn head(cwd: &Path) -> Option<String> {
    read(cwd, &["rev-parse", "HEAD"])
}

pub fn collect(cwd: &Path, base: Option<&str>) -> Option<Evidence> {
    let repo = read(cwd, &["rev-parse", "--show-toplevel"])?;
    let Some(exit) = head(cwd) else {
        return Some(Evidence {
            repo: PathBuf::from(repo),
            base: base.map(str::to_string),
            commits: Vec::new(),
            files: Vec::new(),
            reverted: None,
        });
    };
    let range = base.map(|base| format!("{base}..{exit}")).unwrap_or(exit);
    let commits = read(cwd, &["rev-list", &range])?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let files = read(cwd, &["log", "--name-only", "--pretty=format:", &range])?
        .lines()
        .map(str::trim)
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Some(Evidence {
        repo: PathBuf::from(repo),
        base: base.map(str::to_string),
        commits,
        files,
        reverted: None,
    })
}

pub fn reachable_from_any_branch(repo: &Path, commit: &str) -> Result<bool> {
    let contains = format!("--contains={commit}");
    let output = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            &contains,
            "refs/heads",
        ],
    )?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

fn read(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = git(cwd, args).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git(cwd: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("running git in {}", cwd.display()))
}
