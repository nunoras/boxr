use super::json::{joined_reasoning, joined_text, number_at, text_at};
use super::{Harness, HarnessCommand, HarnessSession, LaunchRequest, StreamEvent, TranscriptEntry};
use crate::atif::{Metrics, ObservationResult, Step, ToolCall};
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Pi;

impl Harness for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn command(&self, request: &LaunchRequest, session: &HarnessSession) -> Result<HarnessCommand> {
        let mut args = vec![
            "--print".to_string(),
            "--mode".to_string(),
            "json".to_string(),
            "--model".to_string(),
            request.model.clone(),
            "--session-id".to_string(),
            session.session_id.clone(),
            "--session-dir".to_string(),
            session.dir.display().to_string(),
        ];
        if let Some(effort) = &request.effort {
            args.push("--thinking".to_string());
            args.push(effort.clone());
        }
        Ok(HarnessCommand {
            program: "pi".to_string(),
            args,
            stdin: Some(request.prompt.clone()),
            env: Vec::new(),
        })
    }

    fn parse_event(&self, line: &str) -> StreamEvent {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return StreamEvent::Ignored;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session") => value
                .get("id")
                .and_then(Value::as_str)
                .map(|id| StreamEvent::SessionStarted {
                    harness_session_id: id.to_string(),
                    harness_version: None,
                    model: None,
                })
                .unwrap_or(StreamEvent::Ignored),
            Some("message_end") => final_message(&value).unwrap_or(StreamEvent::Ignored),
            _ => StreamEvent::Ignored,
        }
    }

    fn transcript(
        &self,
        session: &HarnessSession,
        harness_session_id: &str,
        _account: Option<&Path>,
    ) -> Result<PathBuf> {
        let suffix = format!("_{harness_session_id}.jsonl");
        let entries = fs::read_dir(&session.dir)
            .with_context(|| format!("reading {}", session.dir.display()))?;
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(&suffix) {
                return Ok(entry.path());
            }
        }
        Err(anyhow!(
            "no pi session file ending {suffix} under {}",
            session.dir.display()
        ))
    }

    fn transcript_entry(&self, line: &str) -> Option<TranscriptEntry> {
        let value = serde_json::from_str::<Value>(line).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("message") {
            return None;
        }
        let message = value.get("message")?;
        match message.get("role").and_then(Value::as_str)? {
            "user" => user_entry(&value, message),
            "assistant" => assistant_entry(&value, message),
            "toolResult" => tool_results(message),
            _ => None,
        }
    }
}

fn final_message(value: &Value) -> Option<StreamEvent> {
    let message = value.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let text = joined_text(message.get("content")?.as_array()?);
    (!text.is_empty()).then_some(StreamEvent::FinalMessage {
        text: Some(text),
        error: None,
        limit_hit: false,
    })
}

fn user_entry(entry: &Value, message: &Value) -> Option<TranscriptEntry> {
    let text = match message.get("content")? {
        Value::String(text) => text.clone(),
        Value::Array(parts) => joined_text(parts),
        _ => return None,
    };
    let mut step = Step::new("user", text);
    step.timestamp = text_at(entry, "timestamp");
    Some(TranscriptEntry::Step {
        step: Box::new(step),
        response_id: None,
    })
}

fn assistant_entry(entry: &Value, message: &Value) -> Option<TranscriptEntry> {
    let parts = message.get("content")?.as_array()?;
    let mut step = Step::new("agent", joined_text(parts));
    step.reasoning_content = joined_reasoning(parts);
    let calls = tool_calls(parts);
    if !calls.is_empty() {
        step.tool_calls = Some(calls);
    }
    if step.message.is_empty() && step.reasoning_content.is_none() && step.tool_calls.is_none() {
        return None;
    }
    step.timestamp = text_at(entry, "timestamp");
    step.model_name = model_name(message);
    step.metrics = message.get("usage").and_then(metrics);
    Some(TranscriptEntry::Step {
        step: Box::new(step),
        response_id: text_at(message, "responseId"),
    })
}

fn model_name(message: &Value) -> Option<String> {
    match (text_at(message, "provider"), text_at(message, "model")) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (None, Some(model)) => Some(model),
        _ => None,
    }
}

fn tool_calls(parts: &[Value]) -> Vec<ToolCall> {
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("toolCall"))
        .map(|part| ToolCall {
            tool_call_id: text_at(part, "id").unwrap_or_else(|| "unknown".to_string()),
            function_name: text_at(part, "name").unwrap_or_else(|| "unknown".to_string()),
            arguments: part
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Map::new())),
        })
        .collect()
}

fn tool_results(message: &Value) -> Option<TranscriptEntry> {
    let source_call_id = text_at(message, "toolCallId")?;
    Some(TranscriptEntry::ToolResults(vec![ObservationResult {
        source_call_id,
        content: result_content(message.get("content")),
    }]))
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
    let input = number_at(usage, "input");
    let written = number_at(usage, "cacheWrite");
    let cached = number_at(usage, "cacheRead");
    let output = number_at(usage, "output");
    let reasoning = number_at(usage, "reasoning");
    let mut extra = Map::new();
    if written > 0 {
        extra.insert("cache_write_input_tokens".to_string(), Value::from(written));
    }
    if reasoning > 0 {
        extra.insert("reasoning_tokens".to_string(), Value::from(reasoning));
    }
    Some(Metrics {
        prompt_tokens: Some(input + written + cached),
        completion_tokens: Some(output),
        cached_tokens: Some(cached),
        cost_usd: usage
            .get("cost")
            .and_then(|cost| cost.get("total"))
            .and_then(Value::as_f64),
        extra: (!extra.is_empty()).then_some(extra),
    })
}
