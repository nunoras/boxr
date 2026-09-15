pub mod claude;

use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct HarnessCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    SessionStarted { harness_session_id: String },
    FinalMessage { text: String },
    Ignored,
}

pub trait Harness {
    fn id(&self) -> &'static str;
    fn command(&self, request: &LaunchRequest) -> Result<HarnessCommand>;
    fn parse_event(&self, line: &str) -> StreamEvent;
    fn transcript(&self, harness_session_id: &str) -> Result<PathBuf>;
}

pub fn lookup(id: &str) -> Option<Box<dyn Harness>> {
    match id {
        "claude" => Some(Box::new(claude::ClaudeCode)),
        _ => None,
    }
}

pub fn known_ids() -> &'static [&'static str] {
    &["claude"]
}
