mod common;

use common::*;

const MINIMUM_VERSION: (u64, u64, u64) = (0, 2, 0);
const REQUIRED_COMMANDS: [&str; 5] = ["ps", "status", "wait", "stop", "resume"];
const REQUIRED_FLAGS: [&str; 2] = ["--detach", "--remote"];
const DEPOT_STATES: [&str; 5] = ["running", "finished", "stopped", "interrupted", "failed"];

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
