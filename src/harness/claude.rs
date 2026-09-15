use super::{Harness, HarnessCommand, LaunchRequest, StreamEvent, TranscriptEntry};
use crate::atif::{Metrics, ObservationResult, Step, ToolCall};
use crate::home::config_dir;
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;

pub const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";
pub const DEFAULT_CONFIG_DIR: &str = ".claude";

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
        args.extend(["--output-format", "stream-json", "--verbose", "-p"].map(str::to_string));
        Ok(HarnessCommand {
            program: "claude".to_string(),
            args,
            stdin: Some(request.prompt.clone()),
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
                    harness_version: text_at(&value, "claude_code_version"),
                    model: text_at(&value, "model"),
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
        let projects = config_dir(CONFIG_DIR_ENV, DEFAULT_CONFIG_DIR)
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

    fn transcript_entry(&self, line: &str) -> Option<TranscriptEntry> {
        let value = serde_json::from_str::<Value>(line).ok()?;
        match value.get("type").and_then(Value::as_str) {
            Some("user") => user_entry(&value),
            Some("assistant") => assistant_entry(&value),
            _ => None,
        }
    }
}

fn user_entry(value: &Value) -> Option<TranscriptEntry> {
    let content = value.get("message")?.get("content")?;
    let mut step = match content {
        Value::String(text) => Step::new("user", text.clone()),
        Value::Array(parts) => {
            let results = tool_results(parts);
            if !results.is_empty() {
                return Some(TranscriptEntry::ToolResults(results));
            }
            Step::new("user", joined_text(parts))
        }
        _ => return None,
    };
    step.timestamp = text_at(value, "timestamp");
    Some(TranscriptEntry::Step {
        step: Box::new(step),
        response_id: None,
    })
}

fn assistant_entry(value: &Value) -> Option<TranscriptEntry> {
    let message = value.get("message")?;
    let parts = message.get("content")?.as_array()?;
    let mut step = Step::new("agent", joined_text(parts));
    step.reasoning_content = reasoning(parts);
    let calls = tool_calls(parts);
    if !calls.is_empty() {
        step.tool_calls = Some(calls);
    }
    if step.message.is_empty() && step.reasoning_content.is_none() && step.tool_calls.is_none() {
        return None;
    }
    step.timestamp = text_at(value, "timestamp");
    step.model_name = text_at(message, "model");
    step.reasoning_effort = text_at(value, "effort");
    step.metrics = message.get("usage").and_then(metrics);
    Some(TranscriptEntry::Step {
        step: Box::new(step),
        response_id: text_at(message, "id"),
    })
}

fn joined_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn reasoning(parts: &[Value]) -> Option<String> {
    let thoughts = parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("thinking"))
        .filter_map(|part| part.get("thinking").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!thoughts.is_empty()).then_some(thoughts)
}

fn tool_calls(parts: &[Value]) -> Vec<ToolCall> {
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|part| ToolCall {
            tool_call_id: text_at(part, "id").unwrap_or_else(|| "unknown".to_string()),
            function_name: text_at(part, "name").unwrap_or_else(|| "unknown".to_string()),
            arguments: part
                .get("input")
                .cloned()
                .unwrap_or(Value::Object(Map::new())),
        })
        .collect()
}

fn tool_results(parts: &[Value]) -> Vec<ObservationResult> {
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
        .filter_map(|part| {
            Some(ObservationResult {
                source_call_id: text_at(part, "tool_use_id")?,
                content: result_content(part.get("content")),
            })
        })
        .collect()
}

fn result_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => joined_text(parts),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn metrics(usage: &Value) -> Option<Metrics> {
    let input = number_at(usage, "input_tokens");
    let created = number_at(usage, "cache_creation_input_tokens");
    let cached = number_at(usage, "cache_read_input_tokens");
    let output = number_at(usage, "output_tokens");
    let mut extra = Map::new();
    if created > 0 {
        extra.insert(
            "cache_creation_input_tokens".to_string(),
            Value::from(created),
        );
    }
    Some(Metrics {
        prompt_tokens: Some(input + created + cached),
        completion_tokens: Some(output),
        cached_tokens: Some(cached),
        cost_usd: None,
        extra: (!extra.is_empty()).then_some(extra),
    })
}

fn number_at(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn text_at(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}
