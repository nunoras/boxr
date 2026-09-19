mod common;

use common::*;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fake_ssh() -> PathBuf {
    example_binary("fake-ssh")
}

fn remote_command(harness: &Harness, extra_env: &[(&str, &str)], args: &[&str]) -> Command {
    let bin = harness.root.path().join("bin");
    let mut command = harness.command(args, Some(&bin));
    command.env("BOXR_TEST_SSH", fake_ssh());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
}

#[test]
fn remote_launch_prints_the_remote_session_id_and_skips_the_local_ledger() {
    let harness = Harness::new();
    let args_file = harness.root.path().join("ssh-args.txt");
    let output = remote_command(
        &harness,
        &[
            ("BOXR_FAKE_SSH_ARGS", args_file.to_str().expect("utf8 path")),
            ("BOXR_FAKE_SSH_SESSION", "remote-abc"),
        ],
        &[
            "--remote",
            "box-one",
            "--harness",
            "claude",
            "--model",
            "opus",
            "--effort",
            "high",
            "--kind",
            "review",
            "ship it",
        ],
    )
    .output()
    .expect("boxr runs");
    let stdout = stdout_of(&output);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("id: remote-abc"), "{stdout}");
    assert!(stdout.contains("remote: box-one"), "{stdout}");
    assert!(stdout.contains("status: running"), "{stdout}");
    assert!(
        !harness.boxr_home().join("sessions").exists()
            || fs::read_dir(harness.boxr_home().join("sessions"))
                .map(|entries| entries.count() == 0)
                .unwrap_or(true),
        "remote launch wrote a local session"
    );
    let recorded = fs::read_to_string(&args_file).expect("ssh args");
    assert!(recorded.starts_with("box-one\n"), "{recorded}");
    assert!(recorded.contains("boxr --version"), "{recorded}");
    assert!(
        recorded.contains("--detach")
            && recorded.contains("--harness")
            && recorded.contains("claude")
            && recorded.contains("--model")
            && recorded.contains("opus")
            && recorded.contains("--effort")
            && recorded.contains("high")
            && recorded.contains("--kind")
            && recorded.contains("review")
            && recorded.contains("ship it"),
        "{recorded}"
    );
}

#[test]
fn remote_launch_rejects_a_version_mismatch() {
    let harness = Harness::new();
    let output = remote_command(
        &harness,
        &[("BOXR_FAKE_SSH_VERSION", "0.0.1")],
        &[
            "--remote",
            "box-one",
            "--harness",
            "claude",
            "--model",
            "opus",
            "hi",
        ],
    )
    .output()
    .expect("boxr runs");
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("0.0.1") && stderr.contains(env!("CARGO_PKG_VERSION")),
        "{stderr}"
    );
}

#[test]
fn remote_launch_rejects_a_missing_remote_boxr() {
    let harness = Harness::new();
    let output = remote_command(
        &harness,
        &[("BOXR_FAKE_SSH_NO_BOXR", "1")],
        &[
            "--remote",
            "box-one",
            "--harness",
            "claude",
            "--model",
            "opus",
            "hi",
        ],
    )
    .output()
    .expect("boxr runs");
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unavailable"), "{stderr}");
}

#[test]
fn remote_launch_surfaces_a_failed_remote_detach() {
    let harness = Harness::new();
    let output = remote_command(
        &harness,
        &[("BOXR_FAKE_SSH_FAIL_LAUNCH", "1")],
        &[
            "--remote",
            "box-one",
            "--harness",
            "claude",
            "--model",
            "opus",
            "hi",
        ],
    )
    .output()
    .expect("boxr runs");
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("remote launch"), "{stderr}");
}
