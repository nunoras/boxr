use super::{Harness, HarnessCommand, LaunchRequest, StreamEvent};
use crate::home::user_home;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;

pub const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

pub struct ClaudeCode;

impl Harness for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn command(&self, request: &LaunchRequest) -> Result<HarnessCommand> {
        let mut args = vec!["--model".to_string(), request.model.clone()];
        if let Some(effort) = &request.effort {
            args.push("--effort".to_string());
            args.push(effort.clone());
        }
        args.extend(
            ["--output-format", "stream-json", "--verbose", "-p", "--"].map(str::to_string),
        );
        args.push(request.prompt.clone());
        Ok(HarnessCommand {
            program: "claude".to_string(),
            args,
        })
    }

    fn parse_event(&self, line: &str) -> StreamEvent {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return StreamEvent::Ignored;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("system") if value.get("subtype").and_then(Value::as_str) == Some("init") => value
                .get("session_id")
                .and_then(Value::as_str)
                .map(|id| StreamEvent::SessionStarted {
                    harness_session_id: id.to_string(),
                })
                .unwrap_or(StreamEvent::Ignored),
            Some("result") => value
                .get("result")
                .and_then(Value::as_str)
                .map(|text| StreamEvent::FinalMessage {
                    text: text.to_string(),
                })
                .unwrap_or(StreamEvent::Ignored),
            _ => StreamEvent::Ignored,
        }
    }

    fn transcript(&self, harness_session_id: &str) -> Result<PathBuf> {
        let projects = config_dir()
            .ok_or_else(|| {
                anyhow!("cannot locate the claude config directory; set {CONFIG_DIR_ENV}")
            })?
            .join("projects");
        let wanted = format!("{harness_session_id}.jsonl");
        let entries =
            fs::read_dir(&projects).with_context(|| format!("reading {}", projects.display()))?;
        for entry in entries.flatten() {
            let candidate = entry.path().join(&wanted);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(anyhow!(
            "no transcript {wanted} under {}",
            projects.display()
        ))
    }
}

fn config_dir() -> Option<PathBuf> {
    match env::var_os(CONFIG_DIR_ENV) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => user_home().map(|home| home.join(".claude")),
    }
}
