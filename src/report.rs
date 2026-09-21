use crate::cost;
use crate::fail::{EXIT_LEDGER_FAILED, EXIT_OK, EXIT_SESSION_FAILED};
use crate::home::write_file_atomically;
use crate::output::{one_line, Toon, MESSAGE_LIMIT};
use crate::session::Session;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ledger {
    Recorded(PathBuf),
    Failed(String),
    Interrupted,
}

fn default_headless_mode() -> String {
    "headless".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub status: String,
    pub harness: String,
    pub model: String,
    pub effort: Option<String>,
    #[serde(rename = "harnessSessionId")]
    pub harness_session_id: Option<String>,
    #[serde(default = "default_headless_mode")]
    pub mode: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default, rename = "resumedFrom")]
    pub resumed_from: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, rename = "kindSource")]
    pub kind_source: Option<String>,
    pub start: String,
    pub end: String,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub final_message: Option<String>,
    pub steps: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    #[serde(default, rename = "reasoningTokens")]
    pub reasoning_tokens: u64,
    #[serde(default, rename = "apiEquivalentCost")]
    pub api_equivalent_cost: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default, rename = "costError")]
    pub cost_error: Option<String>,
    pub capture_error: Option<String>,
    pub summary_error: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "stderrTail")]
    pub stderr_tail: Option<String>,
    #[serde(default)]
    pub limit_hit: bool,
    #[serde(default)]
    pub interrupted: bool,
    pub ledger: Ledger,
}

impl Report {
    pub fn succeeded(&self) -> bool {
        self.status == "ok"
    }

    pub fn ledger_failed(&self) -> bool {
        matches!(self.ledger, Ledger::Failed(_))
            || self.capture_error.is_some()
            || self.summary_error.is_some()
    }
}

pub fn exit_code(report: &Report) -> i32 {
    match (report.succeeded(), report.ledger_failed()) {
        (false, _) => EXIT_SESSION_FAILED,
        (true, true) => EXIT_LEDGER_FAILED,
        (true, false) => EXIT_OK,
    }
}

pub fn write(path: &Path, report: &Report) -> Result<()> {
    let text = serde_json::to_string(report).context("encoding the session result")?;
    write_file_atomically(path, &format!("{text}\n"))
}

pub fn state_of(status: &str) -> &'static str {
    match status {
        "ok" => "finished",
        "interrupted" => "interrupted",
        _ => "failed",
    }
}

pub fn render(report: &Report, session: &Session) -> String {
    let mut toon = Toon::new();
    toon.section("session")
        .field("id", &report.id)
        .field("status", &report.status)
        .field("state", state_of(&report.status))
        .field("harness", &report.harness)
        .field("model", &report.model)
        .field(
            "effort",
            report.effort.as_deref().unwrap_or("harness-default"),
        )
        .field(
            "account",
            report.profile.as_deref().unwrap_or("harness-default"),
        )
        .field(
            "harnessSessionId",
            report.harness_session_id.as_deref().unwrap_or("unknown"),
        );
    if let Some(parent) = &report.resumed_from {
        toon.field("resumedFrom", parent);
    }
    if let Some(kind) = &report.kind {
        toon.field("kind", kind);
    }
    toon.number("durationMs", report.duration_ms)
        .number("exitCode", report.exit_code)
        .field(
            "message",
            &one_line(report.final_message.as_deref().unwrap_or(""), MESSAGE_LIMIT),
        );
    if report.interrupted {
        toon.number("interrupted", true);
    }
    if report.limit_hit {
        toon.number("limitHit", true);
    }
    if let Some(error) = &report.error {
        toon.field("error", &one_line(error, MESSAGE_LIMIT));
    }
    if let Some(tail) = &report.stderr_tail {
        toon.field("stderrTail", &one_line(tail, MESSAGE_LIMIT));
    }
    let ledger = toon
        .section("ledger")
        .field(
            "normalized",
            &session.normalized_path().display().to_string(),
        )
        .number("steps", report.steps)
        .number("promptTokens", report.prompt_tokens)
        .number("completionTokens", report.completion_tokens)
        .number("cachedTokens", report.cached_tokens)
        .number("reasoningTokens", report.reasoning_tokens)
        .number(
            "apiEquivalentCost",
            cost::render(report.api_equivalent_cost),
        );
    if let Some(currency) = &report.currency {
        ledger.field("currency", currency);
    }
    ledger.field("summary", &session.summary_path().display().to_string());
    if let Some(error) = &report.cost_error {
        ledger.field("costError", &one_line(error, MESSAGE_LIMIT));
    }
    if let Some(error) = &report.capture_error {
        ledger.field("captureError", &one_line(error, MESSAGE_LIMIT));
    }
    if let Some(error) = &report.summary_error {
        ledger.field("summaryError", &one_line(error, MESSAGE_LIMIT));
    }
    let raw = toon
        .section("raw")
        .field("dir", &session.raw_dir().display().to_string())
        .field("stream", &session.stream_path().display().to_string());
    let stream_line = format!("Read the raw stream at {}", session.stream_path().display());
    let record_line = match &report.ledger {
        Ledger::Recorded(transcript) => {
            raw.field("ledger", "recorded")
                .field("transcript", &transcript.display().to_string());
            format!("Read the raw transcript at {}", transcript.display())
        }
        Ledger::Failed(reason) => {
            raw.field("ledger", "failed")
                .field("ledgerError", &one_line(reason, MESSAGE_LIMIT));
            stream_line
        }
        Ledger::Interrupted => {
            raw.field("ledger", "interrupted");
            stream_line
        }
    };

    let help = if report.succeeded() {
        let mut lines = vec![record_line];
        if report.summary_error.is_none() {
            lines.push(format!(
                "Run `boxr show {}` to read the session summary",
                report.id
            ));
            lines.push(format!(
                "Run `boxr resume {} \"<prompt>\"` to continue it",
                report.id
            ));
        }
        lines
    } else {
        vec![
            format!(
                "Read {} for the harness error output",
                session.stderr_path().display()
            ),
            record_line,
        ]
    };
    toon.list("help", &help);
    toon.render()
}
