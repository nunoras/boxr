use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub struct SummaryFixture<'a> {
    model: &'a str,
    kind: &'a str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    duration_ms: u64,
    cost: Option<f64>,
}

impl<'a> SummaryFixture<'a> {
    pub fn new(
        model: &'a str,
        kind: &'a str,
        prompt_tokens: u64,
        completion_tokens: u64,
        cached_tokens: u64,
        duration_ms: u64,
        cost: Option<f64>,
    ) -> SummaryFixture<'a> {
        SummaryFixture {
            model,
            kind,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
            duration_ms,
            cost,
        }
    }
}

pub fn write_summary_fixture(home: &Path, index: usize, fixture: SummaryFixture<'_>) {
    write_summary_line(
        home,
        json!({
            "id": format!("s-{index}"),
            "harness": "claude",
            "model": fixture.model,
            "effort": "high",
            "profile": "work",
            "mode": "headless",
            "start": "2099-01-01T00:00:00.000Z",
            "end": "2099-01-01T00:00:00.004Z",
            "durationMs": fixture.duration_ms,
            "status": "ok",
            "exitCode": 0,
            "steps": 1,
            "promptTokens": fixture.prompt_tokens,
            "completionTokens": fixture.completion_tokens,
            "cachedTokens": fixture.cached_tokens,
            "reasoningTokens": 0,
            "apiEquivalentCost": fixture.cost,
            "currency": "USD",
            "kind": fixture.kind,
            "kindSource": "declared"
        }),
    );
}

pub fn write_summary_line(home: &Path, line: Value) {
    use std::io::Write;
    fs::create_dir_all(home).expect("boxr home");
    let encoded = serde_json::to_string(&line).expect("summary fixture");
    writeln!(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.join("summary.jsonl"))
            .expect("summary ledger"),
        "{encoded}"
    )
    .expect("summary line");
}
