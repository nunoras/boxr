use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub repo: PathBuf,
    pub branch: Option<String>,
    pub base: String,
    pub commits: Vec<String>,
    pub files: Vec<String>,
    pub reverted: Option<Vec<String>>,
}

pub fn head(cwd: &Path) -> Option<String> {
    read(cwd, &["rev-parse", "HEAD"])
}

pub fn collect(cwd: &Path, base: &str, started_millis: u64) -> Option<Evidence> {
    let repo = read(cwd, &["rev-parse", "--show-toplevel"])?;
    let branch = read(cwd, &["branch", "--show-current"]).filter(|name| !name.is_empty());
    let range = format!("{base}..HEAD");
    let range_commits = revisions(cwd, &["rev-list", &range])?;
    let commits =
        reflog_commits(cwd, base, started_millis).unwrap_or_else(|| range_commits.clone());
    let files = files(cwd, commits.iter().chain(range_commits.iter()));
    Some(Evidence {
        repo: PathBuf::from(repo),
        branch,
        base: base.to_string(),
        commits,
        files,
        reverted: None,
    })
}

fn reflog_commits(cwd: &Path, base: &str, started_millis: u64) -> Option<Vec<String>> {
    let output = read(
        cwd,
        &["reflog", "show", "--format=%H%x00%gs%x00%ct", "HEAD"],
    )?;
    let started_seconds = started_millis / 1_000;
    let mut commits = BTreeSet::new();
    for entry in output.lines() {
        let mut fields = entry.split('\0');
        let (Some(commit), Some(action), Some(timestamp)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Ok(timestamp) = timestamp.parse::<u64>() else {
            continue;
        };
        if timestamp >= started_seconds && commit != base && is_commit_action(action) {
            commits.insert(commit.to_string());
        }
    }
    Some(commits.into_iter().collect())
}

fn is_commit_action(action: &str) -> bool {
    ["commit", "merge", "rebase", "cherry-pick", "revert"]
        .iter()
        .any(|prefix| action.starts_with(prefix))
}

fn revisions(cwd: &Path, args: &[&str]) -> Option<Vec<String>> {
    Some(
        read(cwd, args)?
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn files<'a>(cwd: &Path, commits: impl Iterator<Item = &'a String>) -> Vec<String> {
    commits
        .filter_map(|commit| read(cwd, &["show", "--pretty=format:", "--name-only", commit]))
        .flat_map(|output| {
            output
                .lines()
                .map(str::trim)
                .filter(|file| !file.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let reference = format!("refs/heads/{branch}");
    let output = git(repo, &["rev-parse", "--verify", "--quiet", &reference])?;
    Ok(output.status.success())
}

pub fn reachable_from(repo: &Path, commit: &str, branch: &str) -> Result<bool> {
    let reference = format!("refs/heads/{branch}");
    let output = git(repo, &["merge-base", "--is-ancestor", commit, &reference])?;
    Ok(output.status.success())
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
