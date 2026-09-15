use super::{Harness, HarnessCommand, LaunchRequest, Mode, StreamEvent};
use anyhow::Result;
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
        let events_on_stdout = match request.mode {
            Mode::Headless => {
                args.push("--output-format".to_string());
                args.push("stream-json".to_string());
                args.push("--verbose".to_string());
                args.push("-p".to_string());
                args.push(request.prompt.clone());
                true
            }
            Mode::Interactive => {
                args.push(request.prompt.clone());
                false
            }
        };
        Ok(HarnessCommand {
            program: "claude".to_string(),
            args,
            env: Vec::new(),
            events_on_stdout,
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

    fn transcript(&self, command: &HarnessCommand, harness_session_id: &str) -> Option<PathBuf> {
        let projects = config_dir(command)?.join("projects");
        let wanted = format!("{harness_session_id}.jsonl");
        let entries = fs::read_dir(projects).ok()?;
        for entry in entries.flatten() {
            let candidate = entry.path().join(&wanted);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

fn config_dir(command: &HarnessCommand) -> Option<PathBuf> {
    for (key, value) in &command.env {
        if key == CONFIG_DIR_ENV {
            return Some(PathBuf::from(value));
        }
    }
    if let Some(value) = env::var_os(CONFIG_DIR_ENV) {
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    user_home().map(|home| home.join(".claude"))
}

fn user_home() -> Option<PathBuf> {
    let keys: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME"]
    };
    keys.iter()
        .filter_map(env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}
