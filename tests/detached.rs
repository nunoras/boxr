mod common;

use common::*;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver};
use std::thread::sleep;
use std::time::{Duration, Instant};

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

fn await_running_session(harness: &Harness) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
        if let Some(row) = rows.first() {
            return row.split(' ').next().expect("a session id").to_string();
        }
        assert!(
            Instant::now() < deadline,
            "no session ever showed up in `boxr ps`"
        );
        sleep(Duration::from_millis(50));
    }
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
fn kill_supervisor(pid: u32, force: bool) {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "could not signal supervisor {pid}");
}

#[cfg(windows)]
fn kill_supervisor(pid: u32, _force: bool) {
    let status = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status()
        .expect("taskkill runs");
    assert!(status.success(), "could not kill supervisor {pid}");
}

fn detach(harness: &Harness, prompt: &str) -> String {
    let output = harness.run(&["--detach", "--harness", "claude", "--model", "opus", prompt]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("status: running"), "{stdout}");
    session_id_of(&stdout)
}

fn stream_lines(child: &mut Child) -> Receiver<String> {
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });
    receiver
}

#[test]
fn detaching_returns_the_session_id_while_the_harness_is_still_running() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);

    let id = detach(&harness, "review");
    let harness_pid = harness.harness_pid();
    let status = stdout_of(&harness.run(&["status", &id]));

    assert!(status.contains("status: running"), "{status}");
    assert!(status.contains("harness: claude"), "{status}");
    assert!(status.contains("model: opus"), "{status}");
    assert!(status.contains("steps: "), "{status}");
    let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
    assert!(
        rows.iter().any(|row| row.starts_with(&id)),
        "`boxr ps` did not list {id}: {rows:?}"
    );
    assert!(
        process_alive(harness_pid),
        "the fake harness had already exited"
    );

    harness.kill_hung_harness();
}

#[test]
fn ps_status_and_wait_follow_a_detached_session_to_its_end() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(200);

    let id = detach(&harness, "review");
    let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].starts_with(&id), "{rows:?}");
    assert!(stdout_of(&harness.run(&["status", &id])).contains("status: running"));

    let waited = harness.run(&["wait", &id]);
    let stdout = stdout_of(&waited);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("steps: 4"), "{stdout}");
    assert!(stdout.contains("completionTokens: 915"), "{stdout}");

    assert!(stdout_of(&harness.run(&["status", &id])).contains("status: ok"));
    let after = stdout_of(&harness.run(&["ps"]));
    assert!(ps_sessions(&after).is_empty(), "{after}");
    assert!(after.contains("running: 0"), "{after}");
}

#[test]
fn waiting_prints_what_a_blocking_launch_would_have_printed() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(150);

    let blocking = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    let blocking_stdout = stdout_of(&blocking);
    assert_eq!(blocking.status.code(), Some(0), "{}", stderr_of(&blocking));
    assert!(
        blocking_stdout.contains("ledger: recorded"),
        "{blocking_stdout}"
    );
    assert!(
        blocking_stdout.contains("completionTokens: 915"),
        "{blocking_stdout}"
    );
    let blocking_id = session_id_of(&blocking_stdout);

    let id = detach(&harness, "review");
    let waited = harness.run(&["wait", &id]);
    let waited_stdout = stdout_of(&waited);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    assert_eq!(
        without_session_specifics(&waited_stdout, &id),
        without_session_specifics(&blocking_stdout, &blocking_id)
    );
}

fn without_session_specifics(stdout: &str, id: &str) -> String {
    stdout
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("durationMs: ") {
                return "  durationMs: N".to_string();
            }
            line.replace(id, "s-ID")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn waiting_with_a_timeout_leaves_the_session_running() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(300);

    let id = detach(&harness, "review");
    let timed_out = harness.run(&["wait", "--timeout", "1", &id]);
    assert_eq!(
        timed_out.status.code(),
        Some(6),
        "stdout: {}",
        stdout_of(&timed_out)
    );
    assert!(
        stderr_of(&timed_out).contains("still running"),
        "{}",
        stderr_of(&timed_out)
    );
    assert!(stdout_of(&harness.run(&["status", &id])).contains("status: running"));

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
}

#[test]
fn tailing_streams_the_steps_the_ledger_has_appended_so_far() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(200);

    let id = detach(&harness, "review");
    let mut tail = harness.spawn(&["tail", &id]);
    let lines = stream_lines(&mut tail);

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Ok(line) = lines.recv_timeout(Duration::from_millis(200)) {
            let arrived = line.contains("\"source\":\"agent\"");
            seen.push(line);
            if arrived {
                break;
            }
        }
    }
    assert!(
        seen.iter().any(|line| line.contains("\"source\":\"user\"")),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|line| line.contains("\"source\":\"agent\"")),
        "{seen:?}"
    );
    assert!(
        tail.try_wait().expect("tail status").is_none(),
        "`boxr tail` returned before the session finished"
    );

    let status = tail.wait().expect("tail finishes");
    assert_eq!(status.code(), Some(0));
    while let Ok(line) = lines.recv_timeout(Duration::from_millis(100)) {
        seen.push(line);
    }
    let ledger =
        fs::read_to_string(session_dir(&harness).join("normalized.jsonl")).expect("normalized");
    let streamed = format!("{}\n", seen.join("\n"));
    assert_eq!(streamed, ledger);
    assert!(seen
        .last()
        .expect("a closing line")
        .contains("final_metrics"));
}

#[test]
fn stopping_a_detached_session_ends_the_harness_and_records_an_interruption() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);

    let id = detach(&harness, "review");
    let harness_pid = harness.harness_pid();
    let stopped = harness.run(&["stop", &id]);
    let stdout = stdout_of(&stopped);
    assert_eq!(stopped.status.code(), Some(0), "{}", stderr_of(&stopped));
    assert!(stdout.contains("action: stopped"), "{stdout}");
    assert!(stdout.contains("status: interrupted"), "{stdout}");

    await_process_gone(harness_pid);
    assert_eq!(
        summary_of(&harness.boxr_home(), &id)["status"],
        "interrupted"
    );
    assert!(stdout_of(&harness.run(&["status", &id])).contains("status: interrupted"));

    let again = stdout_of(&harness.run(&["stop", &id]));
    assert!(again.contains("action: already-finished"), "{again}");
}

#[test]
fn killing_the_supervisor_kills_the_harness_and_marks_the_session_interrupted() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);

    let id = detach(&harness, "review");
    let harness_pid = harness.harness_pid();
    let pid = supervisor_pid(&harness, &id);
    kill_supervisor(pid, false);

    await_process_gone(harness_pid);
    assert!(await_status(&harness, &id, "interrupted").contains("status: interrupted"));
    assert_eq!(
        summary_of(&harness.boxr_home(), &id)["status"],
        "interrupted"
    );
}

#[cfg(unix)]
#[test]
fn an_abruptly_killed_supervisor_leaves_no_orphaned_harness() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);

    let id = detach(&harness, "review");
    let harness_pid = harness.harness_pid();
    let pid = supervisor_pid(&harness, &id);
    kill_supervisor(pid, true);

    await_process_gone(harness_pid);
    assert!(await_status(&harness, &id, "interrupted").contains("status: interrupted"));
}

#[test]
fn ps_lists_a_session_launched_in_the_foreground() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);

    let launched = harness.spawn(&["--harness", "claude", "--model", "opus", "review"]);
    let id = await_running_session(&harness);
    assert!(stdout_of(&harness.run(&["status", &id])).contains("status: running"));

    harness.kill_hung_harness();
    let output = launched.wait_with_output().expect("the launch finishes");
    assert!(stdout_of(&output).contains(&id), "{}", stdout_of(&output));
}

#[test]
fn commands_on_an_unknown_session_are_usage_errors() {
    let harness = Harness::new();
    for args in [
        vec!["status", "s-nope"],
        vec!["wait", "s-nope"],
        vec!["tail", "s-nope"],
        vec!["stop", "s-nope"],
    ] {
        let output = harness.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}: {}",
            args.join(" "),
            stdout_of(&output)
        );
        assert!(
            stderr_of(&output).contains("no session s-nope"),
            "{}",
            stderr_of(&output)
        );
    }
}

#[test]
fn detaching_with_a_harness_missing_from_path_is_its_own_exit_code() {
    let harness = Harness::new();
    fs::create_dir_all(harness.root.path().join("empty")).expect("empty dir");
    let output = harness.run_with_path(
        &["--detach", "--harness", "claude", "--model", "opus", "hi"],
        None,
    );
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert!(stderr.contains("was not found on PATH"), "{stderr}");
    assert!(!harness.boxr_home().join("sessions").exists());
}

#[test]
fn a_detached_session_refuses_to_start_without_a_prompt() {
    let harness = Harness::new();
    let output = harness.run(&["--detach", "--harness", "claude", "--model", "opus"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("no prompt given"), "{stderr}");
}

#[test]
fn a_detached_harness_failure_still_records_a_failed_session() {
    let mut harness = Harness::new();
    harness.fail_with("7");

    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    let stdout = stdout_of(&waited);

    assert_eq!(waited.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("exitCode: 7"), "{stdout}");
    assert_eq!(summary_of(&harness.boxr_home(), &id)["status"], "failed");
}

#[test]
fn a_detached_session_against_a_missing_transcript_is_a_ledger_failure() {
    let mut harness = Harness::new();
    harness.withhold_transcript();

    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    let stdout = stdout_of(&waited);

    assert_eq!(waited.status.code(), Some(5), "{stdout}");
    assert!(stdout.contains("ledger: failed"), "{stdout}");
    assert!(stdout.contains("captureError: "), "{stdout}");
}

#[test]
fn the_raw_stream_of_a_detached_session_lands_verbatim_under_the_boxr_home() {
    let harness = Harness::new();

    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let session = harness.boxr_home().join("sessions").join(&id);
    let stream = fs::read_to_string(session.join("raw/stream.jsonl")).expect("stream");
    let fixture = fs::read_to_string(fixture_dir("hello").join("stream.jsonl")).expect("fixture");
    assert_eq!(stream.lines().count(), fixture.lines().count());
    assert!(session.join("raw/transcript.jsonl").is_file());
}

#[test]
fn ps_keeps_listing_a_detached_session_until_it_finishes() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(400);

    let id = detach(&harness, "review");
    for _ in 0..3 {
        let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
        assert!(rows.iter().any(|row| row.starts_with(&id)), "{rows:?}");
        sleep(Duration::from_millis(100));
    }
    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(ps_sessions(&stdout_of(&harness.run(&["ps"]))).is_empty());
}

fn append_summary_line(harness: &Harness, id: &str, status: &str) {
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
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "{line}")
        })
        .expect("append a summary line");
}

fn write_launch_only_session(harness: &Harness, id: &str) {
    let dir = harness.boxr_home().join("sessions").join(id);
    fs::create_dir_all(dir.join("raw")).expect("session dir");
    let launch = serde_json::json!({
        "harness": "claude",
        "model": "opus",
        "prompt": "review",
        "cwd": harness.root.path().join("work"),
        "startedMillis": 1
    })
    .to_string();
    fs::write(dir.join("launch.json"), format!("{launch}\n")).expect("launch.json");
}

#[test]
fn a_live_supervisor_beats_a_premature_summary_line() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_after(9);

    let id = detach(&harness, "review");
    let harness_pid = harness.harness_pid();
    append_summary_line(&harness, &id, "ok");

    let status = stdout_of(&harness.run(&["status", &id]));
    assert!(
        status.contains("status: running"),
        "status finished early while the supervisor was alive:\n{status}"
    );
    let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
    assert!(
        rows.iter().any(|row| row.starts_with(&id)),
        "`boxr ps` hid a live supervisor because of a summary line: {rows:?}"
    );

    let stopped = harness.run(&["stop", &id]);
    let stdout = stdout_of(&stopped);
    assert_eq!(stopped.status.code(), Some(0), "{}", stderr_of(&stopped));
    assert!(
        stdout.contains("action: stopped"),
        "stop took the already-finished path while the supervisor was alive:\n{stdout}"
    );
    await_process_gone(harness_pid);
}

#[test]
fn a_launch_record_without_a_supervisor_is_still_starting() {
    let harness = Harness::new();
    let id = "s-starting";
    write_launch_only_session(&harness, id);

    let status = stdout_of(&harness.run(&["status", id]));
    assert!(
        status.contains("status: running"),
        "launch without supervisor was treated as finished:\n{status}"
    );
    let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
    assert!(
        rows.iter().any(|row| row.starts_with(id)),
        "`boxr ps` skipped a starting session: {rows:?}"
    );
    assert!(
        !harness.boxr_home().join("summary.jsonl").is_file(),
        "a starting session was permanently summarized as interrupted"
    );

    let timed_out = harness.run(&["wait", "--timeout", "1", id]);
    assert_eq!(
        timed_out.status.code(),
        Some(6),
        "wait settled a starting session: {}",
        stdout_of(&timed_out)
    );
    assert!(
        !harness.boxr_home().join("summary.jsonl").is_file(),
        "wait finalized a starting session as interrupted"
    );
}

#[test]
fn a_corrupt_supervisor_record_is_not_treated_as_absence() {
    let harness = Harness::new();
    let id = "s-corrupt";
    write_launch_only_session(&harness, id);
    fs::write(
        harness
            .boxr_home()
            .join("sessions")
            .join(id)
            .join("supervisor.json"),
        "{not-json\n",
    )
    .expect("corrupt supervisor.json");

    let status = harness.run(&["status", id]);
    assert_ne!(
        status.status.code(),
        Some(0),
        "corrupt supervisor.json was soft-ignored: {}",
        stdout_of(&status)
    );
    assert!(
        !harness.boxr_home().join("summary.jsonl").is_file(),
        "corrupt supervisor.json finalized the session as interrupted"
    );
    let rows = ps_sessions(&stdout_of(&harness.run(&["ps"])));
    assert!(
        !rows.iter().any(|row| row.starts_with(id)),
        "corrupt supervisor.json still listed the session as running: {rows:?}"
    );
}

#[test]
fn a_job_guard_failure_fails_the_launch() {
    let harness = Harness::new();
    let pid_file = harness.root.path().join("guard-fail.pid");
    let bin_dir = harness.root.path().join("bin");
    install_orphan_harness(&bin_dir, &pid_file);

    let mut command = harness.command(
        &["--harness", "claude", "--model", "opus", "hello"],
        Some(&bin_dir),
    );
    command.env("BOXR_TEST_FAIL_JOB_GUARD", "1");
    let output = command.output().expect("boxr runs");
    let stderr = stderr_of(&output);
    assert_ne!(
        output.status.code(),
        Some(0),
        "launch continued without a process guard: {stderr}"
    );
    assert!(
        stderr.contains("process guard") || stderr.contains("job object guard"),
        "{stderr}"
    );

    if let Some(pid) = wait_for_optional_pid(&pid_file, Duration::from_secs(2)) {
        await_process_gone(pid);
    } else {
        sleep(Duration::from_millis(500));
        if let Some(pid) = wait_for_optional_pid(&pid_file, Duration::from_millis(200)) {
            assert!(
                !process_alive(pid),
                "guard failure left harness {pid} running"
            );
        }
    }
}

fn install_orphan_harness(bin_dir: &std::path::Path, pid_file: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = format!(
            "#!/bin/sh\necho $$ > {}\nexec sleep 60\n",
            pid_file.display()
        );
        let path = bin_dir.join("claude");
        fs::write(&path, script).expect("orphan harness");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod");
    }
    #[cfg(windows)]
    {
        let script = format!(
            "@echo %RANDOM%%RANDOM% > {pid}\r\n@powershell -NoProfile -Command \"Set-Content -Path '{pid}' -Value $PID; Start-Sleep -Seconds 60\"\r\n",
            pid = pid_file.display()
        );
        fs::remove_file(bin_dir.join("claude.exe")).ok();
        fs::write(bin_dir.join("claude.cmd"), script).expect("orphan harness");
    }
}

fn wait_for_optional_pid(path: &std::path::Path, budget: Duration) -> Option<u32> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                return Some(pid);
            }
        }
        sleep(Duration::from_millis(20));
    }
    None
}
