pub mod claude;
pub mod json;
pub mod pi;

use crate::atif::{ObservationResult, Step};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum LaunchMode {
    Fresh,
    Resume { harness_session_id: String },
}

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub cwd: PathBuf,
    pub mode: LaunchMode,
    pub kind: Option<String>,
    pub kind_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HarnessSession {
    pub session_id: String,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct HarnessCommand {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    pub provider: String,
    pub model: String,
}

impl CatalogModel {
    pub fn full_id(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    SessionStarted {
        harness_session_id: String,
        harness_version: Option<String>,
        model: Option<String>,
    },
    FinalMessage {
        text: Option<String>,
        error: Option<String>,
        limit_hit: bool,
    },
    Recovered,
    Ignored,
}

#[derive(Debug, Clone)]
pub enum TranscriptEntry {
    Step {
        step: Box<Step>,
        response_id: Option<String>,
    },
    ToolResults(Vec<ObservationResult>),
}

pub trait Harness: Send + Sync {
    fn id(&self) -> &'static str;
    fn command(&self, request: &LaunchRequest, session: &HarnessSession) -> Result<HarnessCommand>;
    fn login_command(&self) -> Result<HarnessCommand> {
        Err(anyhow!("harness `{}` has no login command", self.id()))
    }
    fn config_dir_env(&self) -> Option<&'static str> {
        None
    }
    fn models_command(&self) -> Option<HarnessCommand> {
        None
    }
    fn parse_models(&self, _output: &str) -> Result<Vec<CatalogModel>> {
        Err(anyhow!(
            "harness `{}` cannot parse a model catalog",
            self.id()
        ))
    }
    fn parse_event(&self, line: &str) -> StreamEvent;
    fn transcript(
        &self,
        session: &HarnessSession,
        harness_session_id: &str,
        account: Option<&Path>,
    ) -> Result<PathBuf>;
    fn transcript_entry(&self, line: &str) -> Option<TranscriptEntry>;
}

pub fn apply_config_dir(
    command: &mut HarnessCommand,
    harness: &dyn Harness,
    config_dir: Option<&Path>,
) {
    if let (Some(name), Some(dir)) = (harness.config_dir_env(), config_dir) {
        command
            .env
            .push((name.to_string(), dir.display().to_string()));
    }
}

pub fn lookup(id: &str) -> Option<Arc<dyn Harness>> {
    match id {
        "claude" => Some(Arc::new(claude::ClaudeCode)),
        "pi" => Some(Arc::new(pi::Pi)),
        _ => None,
    }
}

pub fn known_ids() -> &'static [&'static str] {
    &["claude", "pi"]
}

pub fn closest_match<I, S>(candidates: I, target: &str) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut best: Option<(usize, String)> = None;
    for candidate in candidates {
        let candidate = candidate.as_ref();
        let distance = edit_distance(candidate, target);
        let replace = match &best {
            None => true,
            Some((best_distance, best_text)) => {
                distance < *best_distance
                    || (distance == *best_distance && candidate < best_text.as_str())
            }
        };
        if replace {
            best = Some((distance, candidate.to_string()));
        }
    }
    best.map(|(_, text)| text)
}

pub fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let substitution = if left_char == right_char { 0 } else { 1 };
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}
