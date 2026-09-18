mod common;

use common::*;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

const HARNESS_SESSION_ID: &str = "11111111-2222-4333-8444-555555555555";

fn session_home(harness: &Harness, id: &str) -> PathBuf {
    harness.boxr_home().join("sessions").join(id)
}

fn launch(harness: &Harness, args: &[&str]) -> String {
    let output = harness.run(args);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    session_id_of(&stdout_of(&output))
}

fn resume(harness: &Harness, id: &str, prompt: &str) -> std::process::Output {
    harness.run_with_fixture("resume", &["resume", id, prompt])
}

#[test]
fn resuming_a_finished_session_continues_it_in_resume_mode() {
    let mut harness = Harness::new();
    harness.record_args();
    harness.write_config(r#"{"defaults":{"harness":"claude","model":"sonnet","effort":"high"}}"#);
    let parent = launch(&harness, &["hello"]);

    let resumed = resume(&harness, &parent, "and now?");
    let stdout = stdout_of(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("harness: claude"), "{stdout}");
    assert!(stdout.contains("model: sonnet"), "{stdout}");
    assert!(stdout.contains("effort: high"), "{stdout}");
    assert!(
        stdout.contains(&format!("resumedFrom: {parent}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("harnessSessionId: {HARNESS_SESSION_ID}")),
        "{stdout}"
    );
    assert!(
        stdout.contains("message: \"follow-up answered by boxr fixture\""),
        "{stdout}"
    );

    let child = session_id_of(&stdout);
    assert_ne!(child, parent, "the continuation reused the original id");

    let args = harness.recorded_args();
    assert!(args.contains(&"--resume".to_string()), "{args:?}");
    assert!(
        args.contains(&HARNESS_SESSION_ID.to_string()),
        "the recorded harness session id was not handed to the harness: {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == "sonnet"),
        "{args:?}"
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--effort" && pair[1] == "high"),
        "{args:?}"
    );
    assert_eq!(harness.recorded_prompt(), "and now?");

    let summary = summary_of(&harness.boxr_home(), &child);
    assert_eq!(summary["effort"], "high");
    assert_eq!(summary["status"], "ok");
}

#[test]
fn the_continuation_records_its_own_steps_linked_to_the_original_session() {
    let harness = Harness::new();
    let parent = launch(
        &harness,
        &["--harness", "claude", "--model", "sonnet", "hello"],
    );

    let resumed = resume(&harness, &parent, "and now?");
    let stdout = stdout_of(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    let child = session_id_of(&stdout);

    let lines = normalized_lines(&session_home(&harness, &child).join("normalized.jsonl"));
    let header = &lines[0];
    assert_eq!(header["extra"]["mode"], "resume");
    assert_eq!(header["extra"]["resumedFrom"], parent.as_str());
    assert_eq!(header["extra"]["harnessSessionId"], HARNESS_SESSION_ID);
    assert_eq!(header["session_id"], child.as_str());

    let steps = &lines[1..lines.len() - 1];
    assert_eq!(
        steps.len(),
        2,
        "the continuation replayed the original steps: {lines:?}"
    );
    assert_eq!(steps[0]["step_id"], 1, "{lines:?}");
    assert_eq!(steps[0]["source"], "user");
    assert_eq!(steps[0]["message"], "and now?");
    assert_eq!(steps[1]["step_id"], 2, "{lines:?}");
    assert_eq!(steps[1]["source"], "agent");
    assert_eq!(steps[1]["message"], "follow-up answered by boxr fixture");
    assert_eq!(
        lines.last().expect("closing line")["final_metrics"]["total_steps"],
        2
    );

    let summary = summary_of(&harness.boxr_home(), &child);
    assert_eq!(summary["mode"], "resume");
    assert_eq!(summary["resumedFrom"], parent.as_str());
    assert_eq!(summary["harnessSessionId"], HARNESS_SESSION_ID);
    assert_eq!(summary["model"], "sonnet");
    assert_eq!(summary["steps"], 2);
    assert_eq!(summary["completionTokens"], 5);
    assert_eq!(summary["cachedTokens"], 100);

    let original = normalized_lines(&session_home(&harness, &parent).join("normalized.jsonl"));
    assert_eq!(original.len(), 4, "{original:?}");
    let original_summary = summary_of(&harness.boxr_home(), &parent);
    assert_eq!(original_summary["mode"], "headless");
    assert_eq!(original_summary["resumedFrom"], Value::Null);
}

#[test]
fn the_continuation_carries_the_recorded_profile() {
    let harness = Harness::new();
    create_profile(&harness, "work");
    let parent = launch(
        &harness,
        &[
            "--harness",
            "claude",
            "--model",
            "sonnet",
            "--account",
            "work",
            "hello",
        ],
    );

    let resumed = resume(&harness, &parent, "and now?");
    let stdout = stdout_of(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    let child = session_id_of(&stdout);

    assert_eq!(summary_of(&harness.boxr_home(), &child)["profile"], "work");
    let lines = normalized_lines(&session_home(&harness, &child).join("normalized.jsonl"));
    assert_eq!(lines[0]["extra"]["profile"], "work");
}

fn create_profile(harness: &Harness, profile: &str) {
    fs::create_dir_all(
        harness
            .boxr_home()
            .join("accounts")
            .join("claude")
            .join(profile),
    )
    .expect("account profile");
}

#[test]
fn a_continuation_can_itself_be_resumed() {
    let harness = Harness::new();
    let parent = launch(
        &harness,
        &["--harness", "claude", "--model", "sonnet", "hello"],
    );
    let child = session_id_of(&stdout_of(&resume(&harness, &parent, "and now?")));

    let again = resume(&harness, &child, "and again?");
    let stdout = stdout_of(&again);
    assert_eq!(again.status.code(), Some(0), "{}", stderr_of(&again));
    assert!(
        stdout.contains(&format!("resumedFrom: {child}")),
        "{stdout}"
    );
    let grandchild = session_id_of(&stdout);

    let lines = normalized_lines(&session_home(&harness, &grandchild).join("normalized.jsonl"));
    assert_eq!(lines[0]["extra"]["resumedFrom"], child.as_str());
    assert_eq!(lines[0]["extra"]["mode"], "resume");
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(lines[1]["step_id"], 1, "{lines:?}");
    assert_eq!(lines[1]["source"], "user");
    assert_eq!(lines[2]["source"], "agent");
    assert_eq!(
        summary_of(&harness.boxr_home(), &grandchild)["resumedFrom"],
        child.as_str()
    );
}

#[test]
fn resuming_an_unknown_session_is_a_clear_error() {
    let harness = Harness::new();
    let output = harness.run(&["resume", "s-nope", "hello"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("no session s-nope"), "{stderr}");
    assert!(stderr.contains("help["), "{stderr}");
}

#[test]
fn resuming_a_still_running_session_is_a_clear_error() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);
    let detached = harness.run(&[
        "--detach",
        "--harness",
        "claude",
        "--model",
        "opus",
        "review",
    ]);
    let stdout = stdout_of(&detached);
    assert_eq!(detached.status.code(), Some(0), "{}", stderr_of(&detached));
    let id = session_id_of(&stdout);
    harness.await_hung_harness();

    let resumed = harness.run(&["resume", &id, "and now?"]);
    let stderr = stderr_of(&resumed);
    assert_eq!(resumed.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("still running"), "{stderr}");
    assert!(stderr.contains(&format!("boxr stop {id}")), "{stderr}");
    assert!(
        stdout_of(&harness.run(&["status", &id])).contains("status: running"),
        "the failed resume ended the running session"
    );

    harness.kill_hung_harness();
}

#[test]
fn resuming_a_session_without_a_harness_session_id_is_a_clear_error() {
    let harness = Harness::new();
    append_summary_line(&harness, "s-noid", "ok");

    let output = harness.run(&["resume", "s-noid", "hello"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("recorded no harness session id"),
        "{stderr}"
    );
}

#[test]
fn resuming_a_session_whose_transcript_is_gone_is_a_clear_error() {
    let mut harness = Harness::new();
    harness.withhold_transcript();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&launched);
    assert_eq!(launched.status.code(), Some(5), "{stdout}");
    let id = session_id_of(&stdout);
    assert_eq!(
        summary_of(&harness.boxr_home(), &id)["harnessSessionId"],
        HARNESS_SESSION_ID
    );

    let resumed = harness.run(&["resume", &id, "and now?"]);
    let stderr = stderr_of(&resumed);
    assert_eq!(resumed.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("transcript"), "{stderr}");
}

#[test]
fn resuming_without_a_prompt_is_a_usage_error() {
    let harness = Harness::new();
    let output = harness.run(&["resume", "s-anything"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout_of(&output));
}

#[test]
fn resume_keeps_the_first_step_when_the_prior_transcript_lacks_a_trailing_newline() {
    let harness = Harness::new();
    let parent = launch(
        &harness,
        &["--harness", "claude", "--model", "sonnet", "hello"],
    );
    strip_trailing_newlines(&harness_transcript(&harness));

    let resumed = resume(&harness, &parent, "and now?");
    let stdout = stdout_of(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    let child = session_id_of(&stdout);

    let lines = normalized_lines(&session_home(&harness, &child).join("normalized.jsonl"));
    let steps = &lines[1..lines.len() - 1];
    assert_eq!(
        steps.len(),
        2,
        "the first continuation step was dropped: {lines:?}"
    );
    assert_eq!(steps[0]["step_id"], 1, "{lines:?}");
    assert_eq!(steps[0]["source"], "user");
    assert_eq!(steps[0]["message"], "and now?");
    assert_eq!(steps[1]["source"], "agent");
}

#[test]
fn a_summary_line_without_resumed_from_still_loads() {
    let harness = Harness::new();
    append_legacy_summary_line(&harness, "s-legacy");

    let output = harness.run(&["show", "s-legacy"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("id: s-legacy"), "{stdout}");
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("mode: headless"), "{stdout}");
    assert!(!stdout.contains("resumedFrom:"), "{stdout}");
}

#[test]
fn a_killed_resume_supervisor_keeps_the_parent_link() {
    let mut harness = Harness::new();
    create_profile(&harness, "work");
    let parent = launch(
        &harness,
        &[
            "--harness",
            "claude",
            "--model",
            "sonnet",
            "--account",
            "work",
            "hello",
        ],
    );

    harness.use_fixture("resume");
    harness.hang_after(1);
    let mut child = harness.spawn(&["resume", &parent, "and now?"]);
    let id = await_running_session(&harness);
    assert_ne!(id, parent, "the continuation reused the original id");

    let pid = supervisor_pid(&harness, &id);
    kill_supervisor(pid);
    let _ = child.wait();
    await_process_gone(pid);

    let status = await_status(&harness, &id, "interrupted");
    assert!(
        status.contains(&format!("resumedFrom: {parent}")),
        "interrupted status lost the parent link:\n{status}"
    );

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["status"], "interrupted");
    assert_eq!(summary["mode"], "resume");
    assert_eq!(summary["profile"], "work");
    assert_eq!(summary["resumedFrom"], parent.as_str());
}

fn harness_transcript(harness: &Harness) -> PathBuf {
    let projects = harness.root.path().join("claude").join("projects");
    let mut paths: Vec<PathBuf> = fs::read_dir(&projects)
        .expect("claude projects")
        .flat_map(|entry| {
            fs::read_dir(entry.expect("project").path())
                .into_iter()
                .flatten()
        })
        .map(|entry| entry.expect("transcript entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect();
    assert_eq!(
        paths.len(),
        1,
        "expected one harness transcript, found {paths:?}"
    );
    paths.pop().expect("one harness transcript")
}

fn strip_trailing_newlines(path: &Path) {
    let mut bytes = fs::read(path).expect("harness transcript");
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    fs::write(path, bytes).expect("rewrite the harness transcript");
}

fn await_running_session(harness: &Harness) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
        if let Some(row) = rows.first() {
            return row.split(',').next().expect("a session id").to_string();
        }
        assert!(
            Instant::now() < deadline,
            "no session ever showed up in `boxr ps`"
        );
        sleep(Duration::from_millis(50));
    }
}

fn await_status(harness: &Harness, id: &str, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let want = format!("status: {expected}");
    loop {
        let stdout = stdout_of(&harness.run(&["status", id]));
        if stdout.contains(&want) {
            return stdout;
        }
        assert!(
            Instant::now() < deadline,
            "session {id} never reported {want}:\n{stdout}"
        );
        sleep(Duration::from_millis(50));
    }
}

fn supervisor_pid(harness: &Harness, id: &str) -> u32 {
    let output = harness.run(&["status", id]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    field_of(&stdout, "pid")
        .parse()
        .expect("the supervisor pid")
}

fn ps_sessions(stdout: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut inside = false;
    for line in stdout.lines() {
        if line.starts_with("sessions[") {
            inside = true;
            continue;
        }
        if line.starts_with("help[") {
            inside = false;
        }
        if inside && !line.trim().is_empty() {
            rows.push(line.trim().to_string());
        }
    }
    rows
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/NH", "/FI", &format!("PID eq {pid}")])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
    }
}

fn await_process_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while process_alive(pid) {
        assert!(Instant::now() < deadline, "process {pid} was never killed");
        sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn kill_supervisor(pid: u32) {
    let status = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "could not kill supervisor {pid}");
}

#[cfg(windows)]
fn kill_supervisor(pid: u32) {
    let status = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status()
        .expect("taskkill runs");
    assert!(status.success(), "could not kill supervisor {pid}");
}

fn append_legacy_summary_line(harness: &Harness, id: &str) {
    fs::create_dir_all(harness.boxr_home()).expect("boxr home");
    let path = harness.boxr_home().join("summary.jsonl");
    let line = serde_json::json!({
        "id": id,
        "harness": "claude",
        "harnessSessionId": "legacy-session",
        "model": "opus",
        "effort": null,
        "profile": null,
        "mode": "headless",
        "start": "2026-01-01T00:00:00.000Z",
        "end": "2026-01-01T00:00:01.000Z",
        "durationMs": 1000,
        "status": "ok",
        "exitCode": 0,
        "steps": 1,
        "promptTokens": 1,
        "completionTokens": 1,
        "cachedTokens": 0
    })
    .to_string();
    assert!(
        !line.contains("resumedFrom"),
        "legacy line must omit resumedFrom: {line}"
    );
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| writeln!(file, "{line}"))
        .expect("append a legacy summary line");
}

fn append_summary_line(harness: &Harness, id: &str, status: &str) {
    fs::create_dir_all(harness.boxr_home()).expect("boxr home");
    let path = harness.boxr_home().join("summary.jsonl");
    let line = serde_json::json!({
        "id": id,
        "harness": "claude",
        "model": "opus",
        "mode": "headless",
        "start": "2026-01-01T00:00:00.000Z",
        "end": "2026-01-01T00:00:01.000Z",
        "durationMs": 1000,
        "status": status,
        "exitCode": 0,
        "steps": 1,
        "promptTokens": 1,
        "completionTokens": 1,
        "cachedTokens": 0
    })
    .to_string();
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| writeln!(file, "{line}"))
        .expect("append a summary line");
}
