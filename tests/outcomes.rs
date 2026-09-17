mod common;

use common::*;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::{Child, Command};

#[test]
fn a_successful_session_records_clear_exit_facts_and_no_git_evidence() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&output)));
    assert_eq!(summary["status"], "ok");
    assert_eq!(summary["exitCode"], 0);
    assert_eq!(summary["interrupted"], false);
    assert_eq!(summary["limitHit"], false);
    assert!(summary["error"].is_null(), "{summary}");
    assert!(summary["verdict"].is_null(), "{summary}");
    assert!(summary["git"].is_null(), "{summary}");
}

#[test]
fn a_failed_session_records_the_harness_error_as_its_own_field() {
    let mut harness = Harness::new();
    harness.use_fixture("error");
    harness.fail_with("1");
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(
        stdout.contains("error: \"API Error (500 internal server error)"),
        "{stdout}"
    );

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["exitCode"], 1);
    assert_eq!(
        summary["error"],
        "API Error (500 internal server error): the request timed out"
    );
    assert_eq!(summary["limitHit"], false);
    assert_eq!(summary["interrupted"], false);
}

#[test]
fn a_limit_hit_session_records_the_limit_separately_from_the_status() {
    let mut harness = Harness::new();
    harness.use_fixture("limit");
    harness.fail_with("1");
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("limitHit: true"), "{stdout}");
    assert!(
        stdout.contains("error: \"Claude usage limit reached"),
        "{stdout}"
    );

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["limitHit"], true);
    assert_eq!(summary["interrupted"], false);
    assert_eq!(
        summary["error"],
        "Claude usage limit reached. Will reset at 5pm (America/Los_Angeles)"
    );
}

#[test]
fn an_interrupted_session_records_the_interruption_flag() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);
    let child = harness.spawn(&["--harness", "claude", "--model", "opus", "review"]);
    harness.await_hung_harness();

    interrupt(&child);

    let output = child.wait_with_output().expect("boxr finishes");
    let stdout = stdout_of(&output);
    assert!(stdout.contains("status: interrupted"), "{stdout}");

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "interrupted");
    assert_eq!(summary["interrupted"], true);
    assert_eq!(summary["limitHit"], false);
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

fn session_in_git_repo(harness: &mut Harness) -> (String, String) {
    harness.git_init();
    fs::write(harness.work_dir().join("notes.txt"), "baseline\n").expect("baseline file");
    harness.git(&["add", "-A"]);
    harness.git(&["commit", "-q", "-m", "baseline"]);
    let base = harness.git_line(&["rev-parse", "HEAD"]);
    fs::write(harness.work_dir().join("more.txt"), "session work\n").expect("session file");
    harness.commit_with("session work by the fake harness");
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let id = session_id_of(&stdout_of(&output));
    (id, base)
}

#[test]
fn a_session_in_a_git_repo_records_the_commits_it_made_and_the_files_it_changed() {
    let mut harness = Harness::new();
    let (id, base) = session_in_git_repo(&mut harness);

    let summary = summary_of(&harness.boxr_home(), &id);
    let evidence = &summary["git"];
    assert_eq!(evidence["branch"], "main");
    assert_eq!(evidence["base"], base.as_str());
    assert_eq!(
        evidence["commits"],
        json!([harness.git_line(&["rev-parse", "HEAD"])])
    );
    assert_eq!(evidence["files"], json!(["more.txt"]));
    assert!(evidence["reverted"].is_null(), "{evidence}");
    let recorded_repo = Path::new(evidence["repo"].as_str().expect("repo path"))
        .canonicalize()
        .expect("canonical repo");
    assert_eq!(
        recorded_repo,
        harness
            .work_dir()
            .canonicalize()
            .expect("canonical work dir")
    );
}

#[test]
fn a_session_in_a_git_repo_that_made_no_commits_records_empty_evidence() {
    let harness = Harness::new();
    harness.git_init();
    fs::write(harness.work_dir().join("notes.txt"), "baseline\n").expect("baseline file");
    harness.git(&["add", "-A"]);
    harness.git(&["commit", "-q", "-m", "baseline"]);

    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness.boxr_home(), &session_id_of(&stdout_of(&output)));
    assert_eq!(summary["git"]["commits"], json!([]));
    assert_eq!(summary["git"]["files"], json!([]));
    assert_eq!(summary["git"]["branch"], "main");
}

#[test]
fn outcome_records_the_caller_verdict_and_note() {
    let harness = Harness::new();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = harness.run(&["outcome", &id, "success", "--note", "shipped it"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("outcome:"), "{stdout}");
    assert!(stdout.contains(&format!("id: {id}")), "{stdout}");
    assert!(stdout.contains("verdict: success"), "{stdout}");
    assert!(stdout.contains("note: \"shipped it\""), "{stdout}");
    assert!(stdout.contains("help[2]:"), "{stdout}");

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["verdict"], "success");
    assert_eq!(summary["verdictNote"], "shipped it");

    let stats = harness.run(&["stats", "--by", "verdict", "--since", "7d"]);
    let stats_stdout = stdout_of(&stats);
    assert_eq!(stats.status.code(), Some(0), "{}", stderr_of(&stats));
    assert!(stats_stdout.contains("success,1,"), "{stats_stdout}");
}

#[test]
fn outcome_overwrites_an_earlier_verdict_and_keeps_an_absent_note() {
    let harness = Harness::new();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let first = harness.run(&["outcome", &id, "success", "--note", "shipped it"]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr_of(&first));
    let second = harness.run(&["outcome", &id, "partial"]);
    assert_eq!(second.status.code(), Some(0), "{}", stderr_of(&second));

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["verdict"], "partial");
    assert_eq!(summary["verdictNote"], "shipped it");
}

#[test]
fn outcome_of_an_unknown_session_is_a_usage_error() {
    let harness = Harness::new();
    let output = harness.run(&["outcome", "s-nope", "success"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("no session s-nope"), "{stderr}");
    assert!(stderr.contains("help["), "{stderr}");
}

#[test]
fn outcome_rejects_an_unknown_verdict() {
    let harness = Harness::new();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = harness.run(&["outcome", &id, "maybe"]);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unknown verdict `maybe`"), "{stderr}");
    assert!(
        stderr.contains("Valid verdicts: success, partial, failed"),
        "{stderr}"
    );
}

#[test]
fn the_revert_check_marks_reset_commits_reverted() {
    let mut harness = Harness::new();
    let (id, base) = session_in_git_repo(&mut harness);
    let commit = harness.git_line(&["rev-parse", "HEAD"]);
    assert_ne!(commit, base);

    harness.git(&["reset", "--hard", &base]);

    let output = harness.run(&["outcome", "--check-reverted", &id]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("reverts:"), "{stdout}");
    assert!(stdout.contains("reverted[1]:"), "{stdout}");
    assert!(stdout.contains(&commit), "{stdout}");

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["git"]["commits"], json!([commit]));
    assert_eq!(summary["git"]["reverted"], json!([commit]));
}

#[test]
fn the_revert_check_keeps_commits_that_survived_on_their_branch() {
    let mut harness = Harness::new();
    let (id, _base) = session_in_git_repo(&mut harness);
    let commit = harness.git_line(&["rev-parse", "HEAD"]);
    fs::write(harness.work_dir().join("later.txt"), "later work\n").expect("later file");
    harness.git(&["add", "-A"]);
    harness.git(&["commit", "-q", "-m", "later work by the test"]);

    let output = harness.run(&["outcome", "--check-reverted", &id]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("reverted[0]:"), "{stdout}");
    assert!(stdout.contains("survived[1]:"), "{stdout}");

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["git"]["reverted"], json!([]));
    assert_eq!(summary["git"]["commits"], json!([commit]));
}

#[test]
fn the_revert_check_marks_all_commits_reverted_when_the_branch_is_gone() {
    let mut harness = Harness::new();
    let (id, _base) = session_in_git_repo(&mut harness);
    let commit = harness.git_line(&["rev-parse", "HEAD"]);
    harness.git(&["checkout", "-q", "--detach", "HEAD"]);
    harness.git(&["branch", "-q", "-D", "main"]);

    let output = harness.run(&["outcome", "--check-reverted", &id]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("reverted[1]:"), "{stdout}");

    let summary = summary_of(&harness.boxr_home(), &id);
    assert_eq!(summary["git"]["reverted"], json!([commit]));
}

#[test]
fn the_revert_check_without_git_evidence_is_a_usage_error() {
    let harness = Harness::new();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = harness.run(&["outcome", "--check-reverted", &id]);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("recorded no git evidence"), "{stderr}");
}

#[test]
fn stats_groups_sessions_by_verdict_and_limit_hit() {
    let harness = Harness::new();
    write_outcome_fixture(&harness, "s-0", None, false, "ok");
    write_outcome_fixture(&harness, "s-1", Some("success"), true, "ok");
    write_outcome_fixture(&harness, "s-2", Some("failed"), false, "failed");

    let output = harness.run(&["stats", "--by", "verdict,limitHit", "--since", "7d"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        stdout.contains("stats[3]{verdict,limitHit,sessions,tokens,durationMs}:"),
        "{stdout}"
    );
    assert!(stdout.contains("none,false,1,"), "{stdout}");
    assert!(stdout.contains("success,true,1,"), "{stdout}");
    assert!(stdout.contains("failed,false,1,"), "{stdout}");
}

#[test]
fn stats_groups_sessions_by_status_and_interruption() {
    let harness = Harness::new();
    write_outcome_fixture(&harness, "s-0", None, false, "ok");
    write_outcome_fixture(&harness, "s-1", None, false, "interrupted");
    write_outcome_fixture(&harness, "s-2", None, false, "failed");
    write_outcome_fixture(&harness, "s-3", None, false, "interrupted");

    let output = harness.run(&["stats", "--by", "status,interrupted", "--since", "7d"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        stdout.contains("stats[3]{status,interrupted,sessions,tokens,durationMs}:"),
        "{stdout}"
    );
    assert!(stdout.contains("ok,false,1,"), "{stdout}");
    assert!(stdout.contains("interrupted,true,2,"), "{stdout}");
    assert!(stdout.contains("failed,false,1,"), "{stdout}");
}

#[test]
fn the_reverted_verdict_survives_a_summary_rewrite() {
    let harness = Harness::new();
    let launched = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let verdict = harness.run(&["outcome", &id, "success"]);
    assert_eq!(verdict.status.code(), Some(0), "{}", stderr_of(&verdict));
    let show = harness.run(&["show", &id]);
    let stdout = stdout_of(&show);
    assert_eq!(show.status.code(), Some(0), "{}", stderr_of(&show));
    assert!(stdout.contains("verdict: success"), "{stdout}");
}

fn write_outcome_fixture(
    harness: &Harness,
    id: &str,
    verdict: Option<&str>,
    limit_hit: bool,
    status: &str,
) {
    let home = harness.boxr_home();
    fs::create_dir_all(&home).expect("boxr home");
    let line = json!({
        "id": id,
        "harness": "claude",
        "model": "sonnet",
        "effort": "high",
        "profile": "work",
        "mode": "headless",
        "start": "2099-01-01T00:00:00.000Z",
        "end": "2099-01-01T00:00:00.004Z",
        "durationMs": 4,
        "status": status,
        "exitCode": 0,
        "steps": 1,
        "promptTokens": 10,
        "completionTokens": 20,
        "cachedTokens": 30,
        "kind": "build",
        "kindSource": "declared",
        "interrupted": status == "interrupted",
        "limitHit": limit_hit,
        "verdict": verdict,
    });
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
