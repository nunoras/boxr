mod common;

use common::*;
use std::thread::sleep;
use std::time::{Duration, Instant};

const MINIMUM_VERSION: (u64, u64, u64) = (0, 2, 0);
const REQUIRED_COMMANDS: [&str; 5] = ["ps", "status", "wait", "stop", "resume"];
const REQUIRED_FLAGS: [&str; 2] = ["--detach", "--remote"];
const DEPOT_STATES: [&str; 5] = ["running", "finished", "stopped", "interrupted", "failed"];
const TOOLS_FINAL_MESSAGE_TIMESTAMP: &str = "2026-09-02T03:04:53.666Z";

fn await_status(harness: &Harness, id: &str, needles: &[&str]) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let stdout = stdout_of(&harness.run(&["status", id]));
        if needles.iter().all(|needle| stdout.contains(needle)) {
            return stdout;
        }
        assert!(
            Instant::now() < deadline,
            "status never showed {needles:?}:\n{stdout}"
        );
        sleep(Duration::from_millis(20));
    }
}

fn detach(harness: &Harness, prompt: &str) -> String {
    let output = harness.run(&["--detach", "--harness", "claude", "--model", "opus", prompt]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    session_id_of(&stdout)
}

fn version(text: &str) -> (u64, u64, u64) {
    let token = text
        .split_whitespace()
        .find(|token| token.contains('.'))
        .expect("a version token");
    let mut parts = token
        .trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .split('.');
    let major = parts.next().expect("major").parse().expect("major");
    let minor = parts.next().expect("minor").parse().expect("minor");
    let patch = parts.next().expect("patch").parse().expect("patch");
    (major, minor, patch)
}

fn mentions(help: &str, needle: &str) -> bool {
    help.lines()
        .flat_map(|line| line.split([',', ' ', '\t']))
        .any(|token| token.trim() == needle)
}

fn state_of(stdout: &str) -> String {
    field_of(stdout, "state")
}

fn activity_of(stdout: &str) -> String {
    field_of(stdout, "lastActivity")
        .trim_matches('"')
        .to_string()
}

#[test]
fn the_reported_version_meets_the_minimum_depot_accepts() {
    let harness = Harness::new();
    let output = harness.run(&["--version"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        version(&stdout) >= MINIMUM_VERSION,
        "{stdout} is below {MINIMUM_VERSION:?}"
    );
}

#[test]
fn help_names_every_command_and_flag_depot_requires() {
    let harness = Harness::new();
    let output = harness.run(&["--help"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    for required in REQUIRED_COMMANDS.iter().chain(REQUIRED_FLAGS.iter()) {
        assert!(mentions(&stdout, required), "{stdout} omits {required}");
    }
}

#[test]
fn status_and_ps_report_the_states_depot_reads() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(200);

    let id = detach(&harness, "review");

    let status = stdout_of(&harness.run(&["status", &id]));
    assert_eq!(state_of(&status), "running", "{status}");

    let ps = stdout_of(&harness.run(&["ps"]));
    assert!(ps.contains("sessions[1]{id,state,harness,model}:"), "{ps}");
    assert!(ps.contains(&format!("  {id},running,claude,opus")), "{ps}");

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(
        stdout_of(&waited).contains("status: ok"),
        "{}",
        stdout_of(&waited)
    );

    let finished = stdout_of(&harness.run(&["status", &id]));
    assert_eq!(state_of(&finished), "finished", "{finished}");
    for state in [state_of(&status), state_of(&finished)] {
        assert!(DEPOT_STATES.contains(&state.as_str()), "{state}");
    }
}

#[test]
fn an_expired_wait_reports_a_running_turn_and_exits_zero() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(400);

    let id = detach(&harness, "review");
    let output = harness.run(&["wait", &id, "--timeout", "1"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("status: running"), "{stdout}");
    assert_eq!(state_of(&stdout), "running", "{stdout}");
}

#[test]
fn wait_reports_a_failed_turn_with_exit_zero() {
    let mut harness = Harness::new();
    harness.fail_with("7");

    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    let stdout = stdout_of(&waited);

    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert_eq!(state_of(&stdout), "failed", "{stdout}");
    assert!(stdout.contains("exitCode: 7"), "{stdout}");
    assert!(
        stdout.contains("error: \"fake claude failing on purpose with exit code 7\""),
        "{stdout}"
    );

    let status = stdout_of(&harness.run(&["status", &id]));
    assert_eq!(state_of(&status), "failed", "{status}");
    assert!(status.contains("exitCode: 7"), "{status}");
    assert!(
        status.contains("error: \"fake claude failing on purpose with exit code 7\""),
        "{status}"
    );

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["exitCode"], 7);
    assert_eq!(
        summary["error"],
        "fake claude failing on purpose with exit code 7"
    );
}

#[test]
fn resume_detached_reports_the_child_in_the_detached_shape() {
    let harness = Harness::new();
    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let resumed = harness.run(&["resume", "--detach", &id, "and now?"]);
    let stdout = stdout_of(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    assert!(stdout.contains("status: running"), "{stdout}");
    assert!(stdout.contains("harness: claude"), "{stdout}");
    assert!(stdout.contains("model: opus"), "{stdout}");
    assert!(stdout.contains(&format!("resumedFrom: {id}")), "{stdout}");
    let child = session_id_of(&stdout);
    assert_ne!(child, id, "the detached resume reused the parent id");

    let waited = harness.run(&["wait", &child]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(
        stdout_of(&waited).contains("status: ok"),
        "{}",
        stdout_of(&waited)
    );

    let status = stdout_of(&harness.run(&["status", &child]));
    assert_eq!(state_of(&status), "finished", "{status}");
    let ps = stdout_of(&harness.run(&["ps"]));
    assert!(ps.contains("sessions[0]{id,state,harness,model}:"), "{ps}");
}

#[test]
fn help_mentions_retry_and_the_detached_resume() {
    let harness = Harness::new();
    let output = harness.run(&["--help"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(mentions(&stdout, "retry"), "{stdout}");
}

#[test]
fn stop_exits_zero() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(400);

    let id = detach(&harness, "review");
    let stopped = harness.run(&["stop", &id]);
    assert_eq!(stopped.status.code(), Some(0), "{}", stderr_of(&stopped));
}

#[test]
fn resume_exits_zero() {
    let harness = Harness::new();

    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let resumed = harness.run(&["resume", &id, "and now?"]);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
}

#[test]
fn resume_reports_a_failed_turn_with_exit_zero() {
    let mut harness = Harness::new();
    harness.fail_with("7");

    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let resumed = harness.run(&["resume", &id, "and now?"]);
    let stdout = stdout_of(&resumed);

    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("exitCode: 7"), "{stdout}");
}

#[test]
fn a_running_session_reports_last_activity_and_the_current_tool() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_for(8, 3000);

    let id = detach(&harness, "review");
    let status = await_status(&harness, &id, &["currentTool: Read"]);
    assert!(status.contains("state: running"), "{status}");
    assert!(status.contains("steps: 1"), "{status}");
    assert!(
        activity_of(&status).as_str() >= TOOLS_FINAL_MESSAGE_TIMESTAMP,
        "{status}"
    );

    let again = stdout_of(&harness.run(&["status", &id]));
    assert!(again.contains("steps: 1"), "{again}");
    assert!(again.contains("currentTool: Read"), "{again}");

    let ps = stdout_of(&harness.run(&["ps"]));
    assert!(ps.contains("sessions[1]{id,state,harness,model}:"), "{ps}");
    assert!(!ps.contains("currentTool"), "{ps}");
    assert!(!ps.contains("lastActivity"), "{ps}");

    let timed_out = harness.run(&["wait", "--timeout", "1", &id]);
    let timed_out_stdout = stdout_of(&timed_out);
    assert_eq!(
        timed_out.status.code(),
        Some(0),
        "{}",
        stderr_of(&timed_out)
    );
    assert!(
        timed_out_stdout.contains("status: running"),
        "{timed_out_stdout}"
    );
    assert!(
        timed_out_stdout.contains("currentTool: Read"),
        "{timed_out_stdout}"
    );

    let waited = harness.run(&["wait", &id]);
    let finished = stdout_of(&waited);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(finished.contains("status: ok"), "{finished}");
    assert!(finished.contains("steps: 4"), "{finished}");
    assert!(!finished.contains("currentTool"), "{finished}");

    let after = stdout_of(&harness.run(&["status", &id]));
    assert!(!after.contains("currentTool"), "{after}");
}

#[test]
fn show_truncates_the_final_message_and_message_prints_it_whole() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");

    let launched = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    assert_eq!(launched.status.code(), Some(0), "{}", stderr_of(&launched));
    let id = session_id_of(&stdout_of(&launched));

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    let full: String = lines
        .iter()
        .rev()
        .find(|step| {
            step["source"] == "agent" && !step["message"].as_str().expect("message").is_empty()
        })
        .expect("a final agent message")["message"]
        .as_str()
        .expect("message")
        .to_string();
    assert!(full.chars().count() > 200, "{full}");
    assert!(full.contains('\n'), "{full}");

    let shown = stdout_of(&harness.run(&["show", &id]));
    let flattened: String = full.split_whitespace().collect::<Vec<_>>().join(" ");
    let kept: String = flattened.chars().take(200).collect();
    assert!(shown.contains("message:"), "{shown}");
    assert!(shown.contains(&format!("text: \"{kept}...\"")), "{shown}");
    assert!(!shown.contains("REVIEW-VERDICT: clean\""), "{shown}");

    let raw = harness.run(&["show", "--message", &id]);
    let raw_stdout = stdout_of(&raw);
    assert_eq!(raw.status.code(), Some(0), "{}", stderr_of(&raw));
    assert!(!raw_stdout.contains("session:"), "{raw_stdout}");
    assert_eq!(raw_stdout.trim_end(), full.trim_end(), "{raw_stdout}");
    assert!(raw_stdout.contains("\n\n"), "{raw_stdout}");
}

#[test]
fn last_activity_tracks_a_growing_transcript_during_a_hung_tool_call() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_for(8, 4000);
    harness.grow_while_hung(100);

    let id = detach(&harness, "review");
    let first = await_status(&harness, &id, &["currentTool: Read"]);
    let first_activity = activity_of(&first);
    sleep(Duration::from_millis(1200));

    let second = stdout_of(&harness.run(&["status", &id]));
    assert!(second.contains("currentTool: Read"), "{second}");
    let second_activity = activity_of(&second);
    assert!(
        second_activity > first_activity,
        "lastActivity stayed at {first_activity} and then {second_activity}"
    );

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
}

#[test]
fn show_message_strips_terminal_control_characters() {
    let mut harness = Harness::new();
    harness.use_fixture("control-chars");

    let launched = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    assert_eq!(launched.status.code(), Some(0), "{}", stderr_of(&launched));
    let id = session_id_of(&stdout_of(&launched));

    let raw = harness.run(&["show", "--message", &id]);
    let text = stdout_of(&raw);
    assert_eq!(raw.status.code(), Some(0), "{}", stderr_of(&raw));
    assert!(text.contains("clean"), "{text}");
    assert!(text.contains("second line"), "{text}");
    assert!(
        !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "{text:?}"
    );
}

#[test]
fn show_reports_an_absent_final_message_as_null() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(8);

    let child = harness.spawn(&["--harness", "claude", "--model", "opus", "review"]);
    harness.kill_hung_harness();
    let output = child.wait_with_output().expect("boxr finishes");
    let id = session_id_of(&stdout_of(&output));

    let shown = stdout_of(&harness.run(&["show", &id]));
    assert!(shown.contains("text: null"), "{shown}");

    let raw = harness.run(&["show", "--message", &id]);
    assert_eq!(raw.status.code(), Some(0), "{}", stderr_of(&raw));
    assert_eq!(stdout_of(&raw), "");
}
