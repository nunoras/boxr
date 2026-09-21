mod common;

use common::*;
use serde_json::{json, Value};
use std::fs;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[test]
fn headless_launch_prints_a_toon_result_and_exits_zero() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("harness: claude"), "{stdout}");
    assert!(stdout.contains("model: sonnet"), "{stdout}");
    assert!(
        stdout.contains("harnessSessionId: 11111111-2222-4333-8444-555555555555"),
        "{stdout}"
    );
    assert!(
        stdout.contains("message: \"hello from boxr fixture\""),
        "{stdout}"
    );
    assert!(stdout.contains("durationMs: "), "{stdout}");
    assert!(stdout.contains("ledger: recorded"), "{stdout}");
    assert!(stdout.contains("help[3]:"), "{stdout}");

    let id_line = stdout
        .lines()
        .find(|line| line.trim_start().starts_with("id: "))
        .expect("session id line");
    assert!(id_line.contains("s-"), "{stdout}");
}

#[test]
fn the_raw_transcript_lands_verbatim_under_the_boxr_home() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let raw = session_dir(&harness).join("raw");
    let copied = fs::read(raw.join("transcript.jsonl")).expect("copied transcript");
    let original =
        fs::read(fixture_dir("hello").join("transcript.jsonl")).expect("fixture transcript");
    assert_eq!(copied, original);

    let stream = fs::read_to_string(raw.join("stream.jsonl")).expect("copied stream");
    let fixture_stream =
        fs::read_to_string(fixture_dir("hello").join("stream.jsonl")).expect("fixture stream");
    assert_eq!(stream.lines().count(), fixture_stream.lines().count());
    assert!(stream.contains("\"type\":\"result\""), "{stream}");
}

#[test]
fn a_harness_failure_is_a_failed_status_and_a_non_zero_exit_code() {
    let mut harness = Harness::new();
    harness.fail_with("7");
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("exitCode: 7"), "{stdout}");

    let stderr_log = fs::read_to_string(session_dir(&harness).join("raw/stderr.log"))
        .expect("captured harness stderr");
    assert!(stderr_log.contains("failing on purpose"), "{stderr_log}");
}

#[test]
fn harness_model_and_effort_fall_back_to_config_defaults() {
    let mut harness = Harness::new();
    harness.record_args();
    harness.write_config(r#"{"defaults":{"harness":"claude","model":"opus","effort":"high"}}"#);
    let output = harness.run(&["hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("model: opus"), "{stdout}");
    assert!(stdout.contains("effort: high"), "{stdout}");

    let args = harness.recorded_args();
    assert!(args.contains(&"opus".to_string()), "{args:?}");
    assert!(args.contains(&"--effort".to_string()), "{args:?}");
    assert!(args.contains(&"stream-json".to_string()), "{args:?}");
}

#[test]
fn a_missing_required_value_is_a_clear_error() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "hello"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("no model given and no default configured"),
        "{stderr}"
    );
    assert!(stderr.contains("help["), "{stderr}");
}

#[test]
fn an_unknown_harness_is_a_usage_error() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "gemini", "--model", "pro", "hello"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unknown harness"), "{stderr}");
}

#[test]
fn a_missing_prompt_is_a_usage_error() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("no prompt given"), "{stderr}");
}

#[test]
fn a_harness_missing_from_path_is_its_own_exit_code() {
    let harness = Harness::new();
    fs::create_dir_all(harness.root.path().join("empty")).expect("empty dir");
    let output = harness.run_with_path(&["--harness", "claude", "--model", "sonnet", "hi"], None);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert!(stderr.contains("was not found on PATH"), "{stderr}");
    assert!(!harness.boxr_home().join("sessions").exists());
}

#[test]
fn an_unrecorded_transcript_is_a_ledger_failure_with_its_own_exit_code() {
    let mut harness = Harness::new();
    harness.withhold_transcript();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(5), "{stdout}");
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("ledger: failed"), "{stdout}");
    assert!(stdout.contains("ledgerError: "), "{stdout}");
    assert!(
        stdout.contains("captureError: \"the harness never wrote a transcript"),
        "{stdout}"
    );
    assert!(!stdout.contains("Read the raw transcript"), "{stdout}");
    assert!(!session_dir(&harness).join("raw/transcript.jsonl").exists());
}

#[test]
fn a_prompt_starting_with_a_dash_reaches_claude_as_the_prompt() {
    let mut harness = Harness::new();
    harness.record_args();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "--", "-h"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    assert_eq!(harness.recorded_prompt(), "-h");
    let args = harness.recorded_args();
    assert!(!args.contains(&"-h".to_string()), "{args:?}");
}

#[test]
fn skill_is_forwarded_as_a_literal_prompt() {
    let mut harness = Harness::new();
    harness.record_args();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "skill"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert_eq!(harness.recorded_prompt(), "skill");
}

fn assert_prompt_reaches_claude_intact(harness: &Harness, prompt: &str) {
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", prompt]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(
        harness.recorded_prompt() == prompt,
        "the prompt changed on the way"
    );
}

#[test]
fn a_multi_line_prompt_reaches_claude_intact() {
    let harness = Harness::new();
    #[cfg(windows)]
    harness.install_cmd_shim();
    assert_prompt_reaches_claude_intact(
        &harness,
        "first line\nsecond line with \"quotes\" & %PATH%\r\nthird line\n",
    );
}

#[test]
fn a_prompt_longer_than_the_cmd_line_limit_reaches_claude_intact() {
    let harness = Harness::new();
    #[cfg(windows)]
    harness.install_cmd_shim();
    let prompt = "boxr long prompt ".repeat(600);
    assert!(prompt.len() > 8191);
    assert_prompt_reaches_claude_intact(&harness, &prompt);
}

#[cfg(windows)]
#[test]
fn a_cmd_shim_on_path_launches_claude() {
    let harness = Harness::new();
    harness.install_cmd_shim();

    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("ledger: recorded"), "{stdout}");
}

#[test]
fn steps_land_in_the_normalized_ledger_while_the_harness_is_still_writing() {
    let mut harness = Harness::new();
    harness.slow_harness(400);
    let mut child = harness.spawn(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let fixture_lines = fs::read_to_string(fixture_dir("hello").join("transcript.jsonl"))
        .expect("fixture transcript")
        .lines()
        .count();

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen_while_writing = false;
    while Instant::now() < deadline && !seen_while_writing {
        if child.try_wait().expect("child status").is_some() {
            break;
        }
        let sessions = harness.boxr_home().join("sessions");
        for entry in fs::read_dir(&sessions).into_iter().flatten().flatten() {
            let Ok(text) = fs::read_to_string(entry.path().join("normalized.jsonl")) else {
                continue;
            };
            if text.contains("\"source\":\"user\"")
                && !text.contains("\"final_metrics\"")
                && harness.harness_transcript_lines() < fixture_lines
            {
                seen_while_writing = true;
            }
        }
        sleep(Duration::from_millis(20));
    }

    let status = child.wait().expect("boxr finishes");
    assert_eq!(status.code(), Some(0));
    assert!(
        seen_while_writing,
        "no normalized step appeared while the harness was still writing its transcript"
    );
}

#[test]
fn the_normalized_ledger_is_a_header_then_atif_steps_then_final_metrics() {
    let harness = Harness::new();
    let output = harness.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--effort",
        "high",
        "hello",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    assert!(lines.len() >= 3, "{lines:?}");

    let header = &lines[0];
    assert_eq!(header["schema_version"], "ATIF-v1.8");
    assert_eq!(header["agent"]["name"], "claude");
    assert_eq!(header["agent"]["version"], "2.1.272");
    assert_eq!(header["extra"]["effort"], "high");
    assert_eq!(
        header["extra"]["harnessSessionId"],
        "11111111-2222-4333-8444-555555555555"
    );

    let closing = lines.last().expect("closing line");
    assert_eq!(
        closing["final_metrics"]["total_steps"],
        lines.len() as i64 - 2
    );
    assert_eq!(closing["final_metrics"]["total_completion_tokens"], 11);

    for (index, step) in lines[1..lines.len() - 1].iter().enumerate() {
        assert_eq!(step["step_id"], (index + 1) as i64, "{step}");
        assert!(step["message"].is_string(), "{step}");
    }
    assert_eq!(lines[1]["source"], "user");
    assert_eq!(lines[2]["source"], "agent");
    assert_eq!(lines[2]["model_name"], "claude-sonnet-5");
    assert_eq!(lines[2]["metrics"]["cached_tokens"], 18538);
}

#[test]
fn declared_core_kind_is_recorded_with_its_source() {
    let harness = Harness::new();
    let output = harness.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--kind",
        "build",
        "hello",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&output)));
    assert_eq!(summary["kind"], "build");
    assert_eq!(summary["kindSource"], "declared");
    let header = &normalized_lines(&session_dir(&harness).join("normalized.jsonl"))[0];
    assert_eq!(header["extra"]["kind"], "build");
    assert_eq!(header["extra"]["kindSource"], "declared");
}

#[test]
fn declared_describe_kind_is_accepted() {
    let harness = Harness::new();
    let output = harness.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--kind",
        "describe",
        "hello",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&output)));
    assert_eq!(summary["kind"], "describe");
    assert_eq!(summary["kindSource"], "declared");
}

#[test]
fn an_unknown_kind_lists_the_valid_kinds() {
    let harness = Harness::new();
    let output = harness.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--kind",
        "unknown",
        "hello",
    ]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unknown kind `unknown`"), "{stderr}");
    assert!(
        stderr.contains(
            "Valid kinds: build, chore, describe, docs, fix, plan, research, review"
        ),
        "{stderr}"
    );
}

#[test]
fn a_custom_kind_from_config_is_accepted() {
    let harness = Harness::new();
    harness.write_config(r#"{"kinds":{"deploy":"Production deployment work"}}"#);
    let output = harness.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--kind",
        "deploy",
        "hello",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&output)));
    assert_eq!(summary["kind"], "deploy");
}

#[test]
fn stats_without_sessions_prints_an_empty_table() {
    let harness = Harness::new();
    let output = harness.run(&["stats", "--by", "model,kind", "--since", "7d"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        stdout.contains(
            "stats[0]{model,kind,currency,sessions,tokens,durationMs,apiEquivalentCost,unpricedSessions}:"
        ),
        "{stdout}"
    );
}

#[test]
fn stats_groups_the_summary_ledger_by_model_and_kind() {
    let harness = Harness::new();
    write_summary_fixture(
        &harness,
        0,
        SummaryFixture::new("opus", "build", 10, 20, 30, 40, Some(0.25)),
    );
    write_summary_fixture(
        &harness,
        1,
        SummaryFixture::new("opus", "build", 1, 2, 3, 4, Some(0.5)),
    );
    write_summary_fixture(
        &harness,
        2,
        SummaryFixture::new("sonnet", "review", 5, 6, 7, 8, None),
    );

    let output = harness.run(&["stats", "--by", "model,kind", "--since", "7d"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        stdout.contains(
            "stats[2]{model,kind,currency,sessions,tokens,durationMs,apiEquivalentCost,unpricedSessions}:"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("opus,build,USD,2,66,44,0.75,0"), "{stdout}");
    assert!(
        stdout.contains("sonnet,review,USD,1,18,8,unknown,1"),
        "{stdout}"
    );
}

#[test]
fn stats_rejects_a_non_finite_cost_total() {
    let harness = Harness::new();
    write_summary_fixture(
        &harness,
        0,
        SummaryFixture::new("sonnet", "build", 1, 2, 3, 4, Some(1e308)),
    );
    write_summary_fixture(
        &harness,
        1,
        SummaryFixture::new("sonnet", "build", 1, 2, 3, 4, Some(1e308)),
    );

    let output = harness.run(&["stats", "--by", "model,kind", "--since", "7d"]);

    assert_ne!(output.status.code(), Some(0), "{}", stdout_of(&output));
    assert!(
        stderr_of(&output)
            .contains("calculating API-equivalent cost total produced a non-finite amount"),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn stats_counts_a_summary_written_before_costs_as_unpriced() {
    let harness = Harness::new();
    write_summary_line(
        &harness,
        json!({
            "id": "s-legacy",
            "harness": "claude",
            "model": "opus",
            "effort": "high",
            "profile": "work",
            "mode": "headless",
            "start": "2099-01-01T00:00:00.000Z",
            "end": "2099-01-01T00:00:00.004Z",
            "durationMs": 4,
            "status": "ok",
            "exitCode": 0,
            "steps": 1,
            "promptTokens": 10,
            "completionTokens": 20,
            "cachedTokens": 30,
            "kind": "build",
            "kindSource": "declared"
        }),
    );

    let output = harness.run(&["stats", "--by", "model", "--since", "7d"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("opus,unknown,1,60,4,unknown,1"), "{stdout}");
}

#[test]
fn stats_groups_ten_thousand_sessions_in_under_a_second() {
    let harness = Harness::new();
    for index in 0..10_000 {
        write_summary_fixture(
            &harness,
            index,
            SummaryFixture::new("sonnet", "build", 1, 2, 3, 4, Some(0.000_002)),
        );
    }

    let started = Instant::now();
    let output = harness.run(&["stats", "--by", "model,kind", "--since", "7d"]);
    let elapsed = started.elapsed();

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(elapsed < Duration::from_secs(1), "stats took {elapsed:?}");
}

struct SummaryFixture<'a> {
    model: &'a str,
    kind: &'a str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    duration_ms: u64,
    cost: Option<f64>,
}

impl<'a> SummaryFixture<'a> {
    fn new(
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

fn write_summary_fixture(harness: &Harness, index: usize, fixture: SummaryFixture<'_>) {
    write_summary_line(
        harness,
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

fn write_summary_line(harness: &Harness, line: Value) {
    let home = harness.boxr_home();
    fs::create_dir_all(&home).expect("boxr home");
    let encoded = serde_json::to_string(&line).expect("summary fixture");
    use std::io::Write;
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

#[test]
fn a_summary_line_records_the_session_with_its_token_counts() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["harness"], "claude");
    assert_eq!(summary["model"], "sonnet");
    assert_eq!(summary["mode"], "headless");
    assert_eq!(summary["status"], "ok");
    assert_eq!(summary["exitCode"], 0);
    assert_eq!(
        summary["harnessSessionId"],
        "11111111-2222-4333-8444-555555555555"
    );
    assert_eq!(summary["steps"], 2);
    assert_eq!(summary["promptTokens"], 43279);
    assert_eq!(summary["completionTokens"], 11);
    assert_eq!(summary["cachedTokens"], 18538);
    assert!(summary["start"].as_str().expect("start").ends_with('Z'));
    assert!(summary["end"].as_str().expect("end").ends_with('Z'));
    assert!(summary["durationMs"].is_u64());
}

const SONNET_PRICES: &str = r#"{"currency":"USD","prices":{"sonnet":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":6.0}}}"#;

#[test]
fn a_summary_records_the_api_equivalent_cost_from_the_price_table() {
    let harness = Harness::new();
    harness.write_config(SONNET_PRICES);
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("reasoningTokens: 0"), "{stdout}");
    assert!(stdout.contains("apiEquivalentCost: 0.055109"), "{stdout}");
    assert!(stdout.contains("currency: USD"), "{stdout}");

    let id = session_id_of(&stdout);
    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["currency"], "USD");
    assert_eq!(summary["reasoningTokens"], 0);
    assert_close(summary["apiEquivalentCost"].as_f64(), 0.055_109_4);

    let shown = harness.run(&["show", &id]);
    let shown_stdout = stdout_of(&shown);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr_of(&shown));
    assert!(
        shown_stdout.contains("apiEquivalentCost: 0.055109"),
        "{shown_stdout}"
    );
    assert!(shown_stdout.contains("currency: USD"), "{shown_stdout}");
}

#[test]
fn a_large_finite_price_keeps_the_completed_session() {
    let harness = Harness::new();
    harness.write_config(
        r#"{"currency":"USD","prices":{"sonnet":{"input":1e307,"output":1e307,"cached":1e307,"reasoning":1e307}}}"#,
    );

    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert!(summary["apiEquivalentCost"].is_number(), "{summary}");
}

#[test]
fn a_pricing_read_failure_keeps_the_completed_session() {
    let mut harness = Harness::new();
    harness.write_config(SONNET_PRICES);
    harness.slow_harness(200);
    let process = harness.spawn(&["--harness", "claude", "--model", "sonnet", "hello"]);

    sleep(Duration::from_millis(100));
    harness.write_config("{invalid");
    let output = process.wait_with_output().expect("boxr exits");
    let stdout = stdout_of(&output);
    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));

    assert_eq!(summary["status"], "ok");
    assert!(summary["apiEquivalentCost"].is_null(), "{summary}");
    assert!(summary["costError"].is_string(), "{summary}");
    assert!(stdout.contains("costError:"), "{stdout}");
}

#[test]
fn a_small_finite_price_remains_nonzero() {
    let harness = Harness::new();
    harness.write_config(
        r#"{"currency":"USD","prices":{"sonnet":{"input":1e-8,"output":0.0,"cached":0.0,"reasoning":0.0}}}"#,
    );

    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);
    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        field_of(&stdout, "apiEquivalentCost")
            .parse::<f64>()
            .is_ok_and(|cost| cost > 0.0),
        "{stdout}"
    );
    assert!(
        summary["apiEquivalentCost"]
            .as_f64()
            .is_some_and(|cost| cost > 0.0),
        "{summary}"
    );
}

#[test]
fn an_underflowed_priced_component_records_a_calculation_error() {
    let harness = Harness::new();
    harness.write_config(
        r#"{"currency":"USD","prices":{"sonnet":{"input":1e-323,"output":0.0,"cached":0.0,"reasoning":0.0}}}"#,
    );

    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);
    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert_eq!(summary["status"], "ok");
    assert!(summary["apiEquivalentCost"].is_null(), "{summary}");
    assert!(summary["costError"].is_string(), "{summary}");

    let shown = harness.run(&["show", &session_id_of(&stdout)]);
    let shown_stdout = stdout_of(&shown);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr_of(&shown));
    assert!(shown_stdout.contains("costError:"), "{shown_stdout}");

    let stats = harness.run(&["stats", "--by", "model", "--since", "7d"]);
    let stats_stdout = stdout_of(&stats);
    assert_eq!(stats.status.code(), Some(0), "{}", stderr_of(&stats));
    assert!(
        stats_stdout.contains("sonnet,USD,1,61828,"),
        "{stats_stdout}"
    );
    assert!(stats_stdout.contains(",unknown,0\n"), "{stats_stdout}");
}

#[test]
fn the_configured_currency_sets_the_recorded_currency_and_amount() {
    let harness = Harness::new();
    harness.write_config(SONNET_PRICES);
    let usd = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let usd_summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&usd)));

    harness.write_config(
        r#"{"currency":"EUR","prices":{"sonnet":{"input":1.8,"output":5.4,"cached":0.27,"reasoning":5.4}}}"#,
    );
    let eur = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let eur_summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&eur)));

    assert_eq!(usd_summary["currency"], "USD");
    assert_close(usd_summary["apiEquivalentCost"].as_f64(), 0.055_109_4);
    assert_eq!(eur_summary["currency"], "EUR");
    assert_close(eur_summary["apiEquivalentCost"].as_f64(), 0.049_598_46);
}

#[test]
fn a_cost_calculation_error_is_not_counted_as_unpriced() {
    let harness = Harness::new();
    write_summary_line(
        &harness,
        json!({
            "id": "s-overflow",
            "harness": "claude",
            "model": "sonnet",
            "effort": "high",
            "profile": "work",
            "mode": "headless",
            "kind": "build",
            "start": "2099-01-01T00:00:00.000Z",
            "end": "2099-01-01T00:00:00.004Z",
            "durationMs": 4,
            "status": "ok",
            "exitCode": 0,
            "steps": 1,
            "promptTokens": 1,
            "completionTokens": 2,
            "cachedTokens": 3,
            "apiEquivalentCost": null,
            "currency": "USD",
            "costError": "calculating API-equivalent cost for sonnet produced a non-finite amount"
        }),
    );

    let output = harness.run(&["stats", "--by", "model,kind", "--since", "7d"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        stdout.contains("sonnet,build,USD,1,6,4,unknown,0"),
        "{stdout}"
    );
}

#[test]
fn an_unpriced_model_records_an_unknown_cost() {
    let harness = Harness::new();
    harness.write_config(
        r#"{"currency":"USD","prices":{"opus":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":6.0}}}"#,
    );
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("apiEquivalentCost: unknown"), "{stdout}");

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert!(summary["apiEquivalentCost"].is_null(), "{summary}");
    assert_eq!(summary["currency"], "USD");

    let stats = harness.run(&["stats", "--by", "model", "--since", "7d"]);
    let stats_stdout = stdout_of(&stats);
    assert_eq!(stats.status.code(), Some(0), "{}", stderr_of(&stats));
    assert!(
        stats_stdout.contains("sonnet,USD,1,61828,"),
        "{stats_stdout}"
    );
    assert!(stats_stdout.contains(",unknown,1\n"), "{stats_stdout}");
}

fn assert_close(recorded: Option<f64>, expected: f64) {
    let recorded = recorded.expect("a recorded cost");
    assert!(
        (recorded - expected).abs() < 1e-9,
        "recorded {recorded}, expected {expected}"
    );
}

#[test]
fn show_prints_the_session_as_toon() {
    let harness = Harness::new();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = harness.run(&["show", &id]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.starts_with("session:"), "{stdout}");
    assert!(stdout.contains(&format!("id: {id}")), "{stdout}");
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("mode: headless"), "{stdout}");
    assert!(stdout.contains("completionTokens: 11"), "{stdout}");
    assert!(stdout.contains("help[2]:"), "{stdout}");
}

#[test]
fn show_of_an_unknown_session_is_a_usage_error() {
    let harness = Harness::new();
    let output = harness.run(&["show", "s-nope"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout_of(&output));
    assert!(
        stderr_of(&output).contains("no session s-nope"),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn export_atif_writes_a_document_that_matches_the_schema() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    let launched = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = harness.run(&["export", "--atif", &id]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("schemaVersion: ATIF-v1.8"), "{stdout}");

    let path = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("path: "))
        .expect("export path");
    let document: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("trajectory")).expect("json");
    assert_valid_atif(&document);
    assert_eq!(document["session_id"], id.as_str());
    assert_eq!(document["extra"]["status"], "ok");
    let observed = document["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .filter(|step| step["observation"].is_object())
        .count();
    assert_eq!(observed, 2, "{document}");
}

#[test]
fn tool_results_fold_into_the_agent_step_that_called_them() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    let output = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    let steps = &lines[1..lines.len() - 1];
    assert_valid_steps(steps);
    let sources = sources_of(steps);
    assert_eq!(sources, ["user", "agent", "agent", "agent"]);
    for (step, call_id, content) in [
        (
            &steps[1],
            "toolu_fixture0000000001",
            "default workflow live run",
        ),
        (
            &steps[2],
            "toolu_fixture0000000002",
            "LIVE-DEFAULT-WORKFLOW.md",
        ),
    ] {
        assert_eq!(step["tool_calls"][0]["tool_call_id"], call_id, "{step}");
        let result = &step["observation"]["results"][0];
        assert_eq!(result["source_call_id"], call_id, "{step}");
        assert!(
            result["content"]
                .as_str()
                .expect("content")
                .contains(content),
            "{step}"
        );
    }
    assert!(steps[3]["message"]
        .as_str()
        .expect("message")
        .contains("REVIEW-VERDICT: clean"));
}

#[test]
fn a_reply_split_across_transcript_lines_counts_its_tokens_once_on_a_step_with_content() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    let output = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["steps"], 4);
    assert_eq!(summary["promptTokens"], 25240 + 28999);
    assert_eq!(summary["completionTokens"], 408 + 507);
    assert_eq!(summary["cachedTokens"], 10023 + 25238);

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    let closing = &lines.last().expect("closing line")["final_metrics"];
    assert_eq!(closing["total_prompt_tokens"], 25240 + 28999);
    assert_eq!(closing["total_completion_tokens"], 408 + 507);
    let steps = &lines[1..lines.len() - 1];
    for step in steps {
        let has_content = !step["message"].as_str().expect("message").is_empty()
            || step["reasoning_content"].is_string()
            || step["tool_calls"].is_array();
        assert!(has_content, "a step with no content: {step}");
    }
    let with_metrics: Vec<i64> = steps
        .iter()
        .filter(|step| step["metrics"].is_object())
        .map(|step| step["step_id"].as_i64().expect("step id"))
        .collect();
    assert_eq!(with_metrics, [2, 4], "{lines:?}");
    assert_eq!(steps[1]["metrics"]["completion_tokens"], 408);
    assert_eq!(steps[3]["metrics"]["completion_tokens"], 507);
}

#[test]
fn a_claude_session_prices_its_thinking_tokens_from_the_price_table() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.write_config(
        r#"{"currency":"USD","prices":{"opus":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":60.0}}}"#,
    );

    let output = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["reasoningTokens"], 553);
    assert_close(summary["apiEquivalentCost"].as_f64(), 0.083_886_3);

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    let closing = &lines.last().expect("closing line")["final_metrics"];
    assert_eq!(closing["extra"]["total_reasoning_tokens"], 553);
    assert_eq!(lines[2]["metrics"]["extra"]["reasoning_tokens"], 257);
    assert_eq!(lines[4]["metrics"]["extra"]["reasoning_tokens"], 296);
}

#[test]
fn a_claude_session_without_thinking_token_details_records_no_reasoning_tokens() {
    let mut harness = Harness::new();
    harness.use_fixture("no-thinking-details");
    harness.write_config(
        r#"{"currency":"USD","prices":{"sonnet":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":60.0}}}"#,
    );

    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["reasoningTokens"], 0);
    assert_close(summary["apiEquivalentCost"].as_f64(), 0.055_109_4);
}

#[test]
fn a_harness_killed_mid_run_still_leaves_a_closed_ledger_and_a_summary() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);
    let child = harness.spawn(&["--harness", "claude", "--model", "opus", "review"]);
    harness.kill_hung_harness();
    let output = child.wait_with_output().expect("boxr finishes");
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    assert_eq!(lines[0]["schema_version"], "ATIF-v1.8");
    let closing = &lines.last().expect("closing line")["final_metrics"];
    let steps = &lines[1..lines.len() - 1];
    assert_valid_steps(steps);
    assert_eq!(closing["total_steps"], steps.len() as i64, "{lines:?}");
    let unanswered = steps
        .iter()
        .filter(|step| step["tool_calls"].is_array() && step.get("observation").is_none())
        .count();
    assert_eq!(unanswered, 2, "{lines:?}");

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    if cfg!(unix) {
        assert!(stdout.contains("status: interrupted"), "{stdout}");
        assert_eq!(summary["status"], "interrupted");
        assert_eq!(summary["exitCode"], 128 + 9);
    } else {
        assert!(stdout.contains("status: failed"), "{stdout}");
        assert_eq!(summary["status"], "failed");
        assert_eq!(summary["exitCode"], 1);
    }
}

#[cfg(unix)]
fn interrupt(boxr: &Child) {
    let status = Command::new("kill")
        .args(["-INT", &boxr.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "could not interrupt boxr");
}

#[cfg(windows)]
fn interrupt(boxr: &Child) {
    const CTRL_BREAK_EVENT: u32 = 1;
    let sent = unsafe { console::GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, boxr.id()) };
    assert_ne!(sent, 0, "could not send Ctrl-Break to boxr");
}

#[test]
fn interrupting_boxr_still_closes_the_ledger_and_marks_the_session_interrupted() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);
    let child = harness.spawn(&["--harness", "claude", "--model", "opus", "review"]);
    harness.await_hung_harness();

    interrupt(&child);

    let output = child.wait_with_output().expect("boxr finishes");
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: interrupted"), "{stdout}");

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    assert_eq!(lines[0]["schema_version"], "ATIF-v1.8");
    let steps = &lines[1..lines.len() - 1];
    assert_valid_steps(steps);
    assert_eq!(
        lines.last().expect("closing line")["final_metrics"]["total_steps"],
        steps.len() as i64,
        "{lines:?}"
    );

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "interrupted");
}

#[cfg(windows)]
#[test]
fn closing_the_console_still_closes_the_ledger_and_marks_the_session_interrupted() {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);
    let child = harness.spawn_in_own_console(&["--harness", "claude", "--model", "opus", "review"]);
    harness.await_hung_harness();

    let closed = Command::new(example_binary("close-console"))
        .arg(child.id().to_string())
        .creation_flags(DETACHED_PROCESS)
        .output()
        .expect("close-console runs");
    assert!(closed.status.success(), "{}", stderr_of(&closed));
    child.wait_with_output().expect("boxr finishes");

    let session = session_dir(&harness);
    let lines = normalized_lines(&session.join("normalized.jsonl"));
    assert_eq!(lines[0]["schema_version"], "ATIF-v1.8");
    let steps = &lines[1..lines.len() - 1];
    assert_valid_steps(steps);
    assert_eq!(
        lines.last().expect("closing line")["final_metrics"]["total_steps"],
        steps.len() as i64,
        "{lines:?}"
    );

    let id = session.file_name().expect("session id").to_string_lossy();
    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["status"], "interrupted");
}

#[test]
fn a_harness_that_exits_non_zero_before_any_result_is_failed_not_interrupted() {
    let mut harness = Harness::new();
    harness.use_fixture("missing");
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["exitCode"], 97);
}

#[test]
fn export_atif_of_a_session_with_no_steps_is_refused() {
    let mut harness = Harness::new();
    harness.use_fixture("missing");
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = harness.run(&["export", "--atif", &id]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout_of(&output));
    assert!(
        stderr_of(&output).contains("has no steps"),
        "{}",
        stderr_of(&output)
    );
    assert!(!session_dir(&harness).join("export").exists());
}

#[test]
fn an_unwritable_summary_still_prints_the_session_as_a_ledger_failure() {
    let harness = Harness::new();
    fs::create_dir_all(harness.boxr_home().join("summary.jsonl")).expect("block the summary");
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(5), "{}", stderr_of(&output));
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("summaryError: "), "{stdout}");
    assert!(stdout.contains("ledger: recorded"), "{stdout}");
    assert!(!stdout.contains("boxr show"), "{stdout}");
    assert!(session_id_of(&stdout).starts_with("s-"), "{stdout}");
}
