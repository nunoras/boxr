mod common;

use common::*;
use std::thread::sleep;
use std::time::Duration;

fn launch(harness: &Harness, prompt: &str) -> String {
    let output = harness.run(&["--harness", "claude", "--model", "opus", prompt]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    session_id_of(&stdout_of(&output))
}

fn list_rows(stdout: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in stdout.lines() {
        if line.starts_with("list[") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        match line.strip_prefix("  ") {
            Some(row) => rows.push(row.split(',').map(str::to_string).collect()),
            None => break,
        }
    }
    rows
}

#[test]
fn list_prints_the_newest_session_first_and_honours_the_limit() {
    let harness = Harness::new();
    let first = launch(&harness, "one");
    sleep(Duration::from_millis(10));
    let second = launch(&harness, "two");
    sleep(Duration::from_millis(10));
    let third = launch(&harness, "three");

    let limited = stdout_of(&harness.run(&["list", "--limit", "2"]));
    let rows = list_rows(&limited);
    assert!(limited.contains("list[2]{"), "{limited}");
    assert_eq!(rows.len(), 2, "{limited}");
    assert_eq!(rows[0][0], third, "{limited}");
    assert_eq!(rows[1][0], second, "{limited}");

    let all = list_rows(&stdout_of(&harness.run(&["list"])));
    assert_eq!(all.len(), 3, "{all:?}");
    assert_eq!(all[0][0], third, "{all:?}");
    assert_eq!(all[2][0], first, "{all:?}");

    let explicit_with_all = list_rows(&stdout_of(&harness.run(&["list", "--all", "--limit", "1"])));
    assert_eq!(explicit_with_all.len(), 1, "{explicit_with_all:?}");
    assert_eq!(explicit_with_all[0][0], third, "{explicit_with_all:?}");
}

#[test]
fn list_folds_the_verdict_recorded_by_outcome() {
    let harness = Harness::new();
    let id = launch(&harness, "hello");

    let recorded = harness.run(&["outcome", &id, "failed", "--note", "still red"]);
    assert_eq!(recorded.status.code(), Some(0), "{}", stderr_of(&recorded));

    let rows = list_rows(&stdout_of(&harness.run(&["list"])));
    let row = rows
        .iter()
        .find(|row| row[0] == id)
        .unwrap_or_else(|| panic!("no row for {id}: {rows:?}"));
    assert_eq!(row[1], "finished", "{row:?}");
    assert_eq!(row[4], "ok", "{row:?}");
    assert_eq!(row[8], "failed", "{row:?}");
}

#[test]
fn list_includes_a_running_session_once() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(400);

    let output = harness.run(&[
        "--detach",
        "--harness",
        "claude",
        "--model",
        "opus",
        "review",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let id = session_id_of(&stdout_of(&output));

    let running = list_rows(&stdout_of(&harness.run(&["list"])));
    let matches: Vec<&Vec<String>> = running.iter().filter(|row| row[0] == id).collect();
    assert_eq!(matches.len(), 1, "{running:?}");
    assert_eq!(matches[0][1], "running", "{running:?}");
    assert_eq!(matches[0][4], "running", "{running:?}");

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let finished = list_rows(&stdout_of(&harness.run(&["list"])));
    let matches: Vec<&Vec<String>> = finished.iter().filter(|row| row[0] == id).collect();
    assert_eq!(matches.len(), 1, "{finished:?}");
    assert_eq!(matches[0][1], "finished", "{finished:?}");
    assert_eq!(matches[0][4], "ok", "{finished:?}");
}

#[test]
fn an_unknown_single_word_is_refused_with_the_closest_command() {
    let harness = Harness::new();
    let output = harness.run(&["statsu"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("`statsu` is not a boxr command"),
        "{stderr}"
    );
    assert!(stderr.contains("Did you mean `stats`?"), "{stderr}");
    assert!(!harness.boxr_home().join("sessions").exists());

    let typo = harness.run(&["lis"]);
    let stderr = stderr_of(&typo);
    assert_eq!(typo.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("Did you mean `list`?"), "{stderr}");
}

#[test]
fn a_word_that_is_far_from_every_command_gets_no_did_you_mean() {
    let harness = Harness::new();
    let output = harness.run(&["hello"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("`hello` is not a boxr command"), "{stderr}");
    assert!(!stderr.contains("Did you mean"), "{stderr}");
    assert!(
        stderr.contains("pass `--harness <h> --model <m>`, or quote a longer prompt"),
        "{stderr}"
    );
    assert!(!harness.boxr_home().join("sessions").exists());
}

#[test]
fn an_explicit_single_word_prompt_still_launches() {
    let harness = Harness::new();
    let id = launch(&harness, "hello");
    assert!(id.starts_with("s-"), "{id}");
}

#[test]
fn a_multiword_prompt_still_uses_the_configured_defaults() {
    let harness = Harness::new();
    harness.write_config(r#"{"defaults":{"harness":"claude","model":"opus"}}"#);

    let output = harness.run(&["hello there"]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("model: opus"), "{stdout}");
}

#[test]
fn a_single_bare_word_is_refused_even_with_configured_defaults() {
    let harness = Harness::new();
    harness.write_config(r#"{"defaults":{"harness":"claude","model":"opus"}}"#);

    let output = harness.run(&["hello"]);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("`hello` is not a boxr command"), "{stderr}");
    assert!(!harness.boxr_home().join("sessions").exists());
}
