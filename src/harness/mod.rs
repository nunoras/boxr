pub mod claude;
pub mod json;
pub mod pi;

use crate::atif::{ObservationResult, Step};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub cwd: PathBuf,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    SessionStarted {
        harness_session_id: String,
        harness_version: Option<String>,
        model: Option<String>,
    },
    FinalMessage {
        text: String,
    },
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
    fn parse_event(&self, line: &str) -> StreamEvent;
    fn transcript(&self, session: &HarnessSession, harness_session_id: &str) -> Result<PathBuf>;
    fn transcript_entry(&self, line: &str) -> Option<TranscriptEntry>;
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
