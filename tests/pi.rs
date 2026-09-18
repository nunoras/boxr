mod common;

use common::{
    assert_valid_atif, assert_valid_steps, example_binary, normalized_lines, session_id_of,
    sources_of, stderr_of, stdout_of, summary_of,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Pi {
    root: tempfile::TempDir,
    bin_dir: PathBuf,
    args_file: PathBuf,
    prompt_file: PathBuf,
    exit_code: String,
    fixture: &'static str,
}

impl Pi {
    fn new() -> Pi {
        let root = tempfile::tempdir().expect("temp dir");
        let bin_dir = root.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("bin dir");
        fs::create_dir_all(root.path().join("work")).expect("work dir");
        fs::copy(example_binary("fake-pi"), bin_dir.join(fake_name())).expect("install fake pi");
        let args_file = root.path().join("pi-args.txt");
        let prompt_file = root.path().join("pi-prompt.txt");
        Pi {
            root,
            bin_dir,
            args_file,
            prompt_file,
            exit_code: "0".to_string(),
            fixture: "hello",
        }
    }

    fn boxr_home(&self) -> PathBuf {
        self.root.path().join("boxr")
    }

    fn session_dir(&self) -> PathBuf {
        let sessions = self.boxr_home().join("sessions");
        let mut entries: Vec<PathBuf> = fs::read_dir(&sessions)
            .expect("sessions dir")
            .map(|entry| entry.expect("entry").path())
            .collect();
        entries.sort();
        entries.pop().expect("one session directory")
    }

    fn harness_dir(&self) -> PathBuf {
        self.session_dir().join("harness")
    }

    fn harness_session_file(&self) -> PathBuf {
        let dir = self.harness_dir();
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .expect("harness session dir")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
            })
            .collect();
        files.sort();
        assert_eq!(files.len(), 1, "one pi session file in {}", dir.display());
        files.pop().expect("the pi session file")
    }

    fn use_fixture(&mut self, name: &'static str) {
        self.fixture = name;
    }

    fn fail_with(&mut self, code: &str) {
        self.exit_code = code.to_string();
    }

    fn recorded_args(&self) -> Vec<String> {
        fs::read_to_string(&self.args_file)
            .expect("recorded args")
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn recorded_prompt(&self) -> String {
        fs::read_to_string(&self.prompt_file).expect("recorded prompt")
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_fixture(self.fixture, args)
    }

    fn run_with_fixture(&self, fixture: &str, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxr"));
        command
            .args(args)
            .current_dir(self.root.path().join("work"))
            .env("BOXR_HOME", self.boxr_home())
            .env("PATH", &self.bin_dir)
            .env("BOXR_FAKE_PI_FIXTURE", fixture_dir(fixture))
            .env("BOXR_FAKE_PI_EXIT", &self.exit_code)
            .env("BOXR_FAKE_PI_ARGS", &self.args_file)
            .env("BOXR_FAKE_PI_PROMPT", &self.prompt_file);
        command.output().expect("boxr runs")
    }
}

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi")
        .join(name)
}

fn fake_name() -> &'static str {
    if cfg!(windows) {
        "pi.exe"
    } else {
        "pi"
    }
}

fn arg_after(args: &[String], name: &str) -> String {
    let index = args
        .iter()
        .position(|arg| arg == name)
        .unwrap_or_else(|| panic!("{name} was not passed to pi: {args:?}"));
    args.get(index + 1)
        .unwrap_or_else(|| panic!("{name} has no value in {args:?}"))
        .clone()
}

fn steps_of(lines: &[Value]) -> &[Value] {
    &lines[1..lines.len() - 1]
}

#[test]
fn pi_launch_prints_a_toon_result_and_exits_zero() {
    let pi = Pi::new();
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("harness: pi"), "{stdout}");
    assert!(stdout.contains("model: xai/grok-4.5"), "{stdout}");
    assert!(stdout.contains("effort: harness-default"), "{stdout}");
    assert!(
        stdout.contains("message: \"hello from boxr fixture\""),
        "{stdout}"
    );
    assert!(stdout.contains("durationMs: "), "{stdout}");
    assert!(stdout.contains("ledger: recorded"), "{stdout}");
    assert!(stdout.contains("help[3]:"), "{stdout}");
    assert!(session_id_of(&stdout).starts_with("s-"), "{stdout}");
}

#[test]
fn boxr_tells_pi_the_session_id_and_directory_to_use() {
    let pi = Pi::new();
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let args = pi.recorded_args();
    assert!(args.contains(&"--print".to_string()), "{args:?}");
    assert!(args.contains(&"--mode".to_string()), "{args:?}");
    assert!(args.contains(&"json".to_string()), "{args:?}");
    assert!(args.contains(&"xai/grok-4.5".to_string()), "{args:?}");

    let id = arg_after(&args, "--session-id");
    assert_eq!(id, session_id_of(&stdout), "{stdout}");
    assert!(
        stdout.contains(&format!("harnessSessionId: {id}")),
        "{stdout}"
    );
    assert_eq!(
        arg_after(&args, "--session-dir"),
        pi.harness_dir().display().to_string()
    );

    let written = pi.harness_session_file();
    assert!(
        written
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .ends_with(&format!("_{id}.jsonl")),
        "{}",
        written.display()
    );
    assert_eq!(
        fs::read(pi.session_dir().join("raw/transcript.jsonl")).expect("copied transcript"),
        fs::read(&written).expect("the pi session file")
    );
}

#[test]
fn pi_transcripts_normalize_into_atif_steps_with_token_counts() {
    let pi = Pi::new();
    let output = pi.run(&[
        "--harness",
        "pi",
        "--model",
        "xai/grok-4.5",
        "--effort",
        "high",
        "hello",
    ]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let lines = normalized_lines(&pi.session_dir().join("normalized.jsonl"));
    let header = &lines[0];
    assert_eq!(header["schema_version"], "ATIF-v1.8");
    assert_eq!(header["agent"]["name"], "pi");
    assert_eq!(header["agent"]["model_name"], "xai/grok-4.5");
    assert_eq!(header["extra"]["effort"], "high");
    assert_eq!(header["extra"]["mode"], "headless");

    let steps = steps_of(&lines);
    assert_valid_steps(steps);
    assert_eq!(sources_of(steps), ["user", "agent"]);
    assert_eq!(
        steps[0]["message"],
        "Reply with exactly: hello from boxr fixture"
    );
    assert_eq!(steps[1]["model_name"], "xai/grok-4.5");
    assert_eq!(steps[1]["message"], "hello from boxr fixture");
    assert_eq!(
        steps[1]["reasoning_content"],
        "The user wants me to reply with exactly: hello from boxr fixture"
    );
    assert_eq!(steps[1]["metrics"]["prompt_tokens"], 8769);
    assert_eq!(steps[1]["metrics"]["completion_tokens"], 19);
    assert_eq!(steps[1]["metrics"]["cached_tokens"], 384);

    let closing = &lines.last().expect("closing line")["final_metrics"];
    assert_eq!(closing["total_steps"], 2);
    assert_eq!(closing["total_prompt_tokens"], 8769);
    assert_eq!(closing["total_completion_tokens"], 19);
    assert_eq!(closing["total_cached_tokens"], 384);
    assert_eq!(closing["extra"]["total_reasoning_tokens"], 14);

    let summary = summary_of(&pi.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["harness"], "pi");
    assert_eq!(summary["model"], "xai/grok-4.5");
    assert_eq!(summary["effort"], "high");
    assert_eq!(summary["status"], "ok");
    assert_eq!(summary["steps"], 2);
    assert_eq!(summary["promptTokens"], 8769);
    assert_eq!(summary["completionTokens"], 19);
    assert_eq!(summary["cachedTokens"], 384);
    assert_eq!(summary["reasoningTokens"], 14);
}

#[test]
fn a_pi_session_prices_its_reasoning_tokens_from_the_price_table() {
    let pi = Pi::new();
    let home = pi.boxr_home();
    fs::create_dir_all(&home).expect("boxr home");
    fs::write(
        home.join("config.json"),
        r#"{"currency":"USD","prices":{"xai/grok-4.5":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":60.0}}}"#,
    )
    .expect("config");

    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let summary = summary_of(&home, &session_id_of(&stdout));
    assert_eq!(summary["reasoningTokens"], 14);
    assert_eq!(summary["currency"], "USD");
    let cost = summary["apiEquivalentCost"]
        .as_f64()
        .expect("a recorded cost");
    assert!((cost - 0.017_755_2).abs() < 1e-9, "{cost}");
}

#[test]
fn a_pi_session_exports_as_a_valid_atif_trajectory() {
    let pi = Pi::new();
    let launched = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let id = session_id_of(&stdout_of(&launched));

    let output = pi.run(&["export", "--atif", &id]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("schemaVersion: ATIF-v1.8"), "{stdout}");
    assert!(stdout.contains("steps: 2"), "{stdout}");

    let path = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("path: "))
        .expect("export path");
    let document: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("trajectory")).expect("json");
    assert_valid_atif(&document);
    assert_eq!(document["agent"]["name"], "pi");
    assert_eq!(document["session_id"], id.as_str());
}

#[test]
fn pi_tool_results_fold_into_the_agent_step_that_called_them() {
    let mut pi = Pi::new();
    pi.use_fixture("tools");
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "review"]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let lines = normalized_lines(&pi.session_dir().join("normalized.jsonl"));
    let steps = steps_of(&lines);
    assert_valid_steps(steps);
    assert_eq!(sources_of(steps), ["user", "agent", "agent", "agent"]);
    for (step, content) in [
        (&steps[1], "boxr-tool-fixture-one"),
        (&steps[2], "boxr-tool-fixture-two"),
    ] {
        let call_id = step["tool_calls"][0]["tool_call_id"]
            .as_str()
            .expect("tool_call_id");
        assert_eq!(step["tool_calls"][0]["function_name"], "bash", "{step}");
        assert!(
            step["tool_calls"][0]["arguments"]["command"]
                .as_str()
                .expect("command")
                .contains(content),
            "{step}"
        );
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

    let summary = summary_of(&pi.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["steps"], 4);
    assert_eq!(summary["promptTokens"], 26647);
    assert_eq!(summary["completionTokens"], 131);
    assert_eq!(summary["cachedTokens"], 9216);
}

#[test]
fn pi_receives_the_prompt_on_stdin() {
    let pi = Pi::new();
    let prompt = "first line\nsecond line with \"quotes\" & %PATH%\r\nthird line\n";
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", prompt]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert_eq!(pi.recorded_prompt(), prompt);
}

#[test]
fn a_pi_harness_failure_is_a_failed_status_and_a_non_zero_exit_code() {
    let mut pi = Pi::new();
    pi.fail_with("7");
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("exitCode: 7"), "{stdout}");

    let stderr_log = fs::read_to_string(pi.session_dir().join("raw/stderr.log"))
        .expect("captured harness stderr");
    assert!(stderr_log.contains("failing on purpose"), "{stderr_log}");
}

#[test]
fn a_pi_turn_error_is_recorded_even_when_the_process_exits_zero() {
    let mut pi = Pi::new();
    pi.use_fixture("error");
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("exitCode: 0"), "{stdout}");
    assert!(
        stdout.contains("error: \"API error (500): the request timed out\""),
        "{stdout}"
    );

    let summary = summary_of(&pi.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["exitCode"], 0);
    assert_eq!(summary["limitHit"], false);
    assert_eq!(summary["error"], "API error (500): the request timed out");
}

#[test]
fn a_pi_usage_limit_is_recorded_as_limit_hit() {
    let mut pi = Pi::new();
    pi.use_fixture("limit");
    let output = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: failed"), "{stdout}");
    assert!(stdout.contains("limitHit: true"), "{stdout}");
    assert!(stdout.contains("Weekly usage limit reached"), "{stdout}");

    let summary = summary_of(&pi.boxr_home(), &session_id_of(&stdout));
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["limitHit"], true);
    assert_eq!(summary["interrupted"], false);
    assert!(
        summary["error"]
            .as_str()
            .expect("error")
            .contains("Weekly usage limit reached"),
        "{summary:?}"
    );
}

#[test]
fn resuming_a_pi_session_continues_the_original_harness_transcript() {
    let pi = Pi::new();
    let parent_out = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let parent_stdout = stdout_of(&parent_out);
    assert_eq!(
        parent_out.status.code(),
        Some(0),
        "{}",
        stderr_of(&parent_out)
    );
    let parent = session_id_of(&parent_stdout);
    let parent_harness_id = parent.clone();
    let parent_harness_dir = pi
        .boxr_home()
        .join("sessions")
        .join(&parent)
        .join("harness");
    let parent_transcript = session_file_in(&parent_harness_dir, &parent_harness_id);
    let parent_size = fs::metadata(&parent_transcript)
        .expect("parent transcript")
        .len();

    let resumed = pi.run_with_fixture("resume", &["resume", &parent, "and now?"]);
    let stdout = stdout_of(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{}", stderr_of(&resumed));
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(
        stdout.contains(&format!("resumedFrom: {parent}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("harnessSessionId: {parent_harness_id}")),
        "{stdout}"
    );
    assert!(
        stdout.contains("message: \"follow-up answered by boxr fixture\""),
        "{stdout}"
    );

    let args = pi.recorded_args();
    assert_eq!(arg_after(&args, "--session-id"), parent_harness_id);
    assert_eq!(
        arg_after(&args, "--session-dir"),
        parent_harness_dir.display().to_string()
    );

    let child = session_id_of(&stdout);
    assert_ne!(child, parent);
    let summary = summary_of(&pi.boxr_home(), &child);
    assert_eq!(summary["mode"], "resume");
    assert_eq!(summary["resumedFrom"], parent.as_str());
    assert_eq!(summary["harnessSessionId"], parent_harness_id.as_str());
    assert_eq!(summary["steps"], 2);

    let continued = session_file_in(&parent_harness_dir, &parent_harness_id);
    let continued_size = fs::metadata(&continued)
        .expect("continued transcript")
        .len();
    assert!(
        continued_size > parent_size,
        "{continued_size} <= {parent_size}"
    );

    let lines = normalized_lines(
        &pi.boxr_home()
            .join("sessions")
            .join(&child)
            .join("normalized.jsonl"),
    );
    assert_eq!(lines[0]["extra"]["mode"], "resume");
    assert_eq!(lines[0]["extra"]["resumedFrom"], parent.as_str());
    let steps = steps_of(&lines);
    assert_eq!(sources_of(steps), ["user", "agent"]);
    assert_eq!(steps[0]["message"], "and now?");
    assert_eq!(steps[1]["message"], "follow-up answered by boxr fixture");
}

#[test]
fn resuming_a_pi_continuation_keeps_the_origin_harness_dir() {
    let pi = Pi::new();
    let parent_out = pi.run(&["--harness", "pi", "--model", "xai/grok-4.5", "hello"]);
    let parent_stdout = stdout_of(&parent_out);
    assert_eq!(
        parent_out.status.code(),
        Some(0),
        "{}",
        stderr_of(&parent_out)
    );
    let parent = session_id_of(&parent_stdout);
    let origin_harness_id = parent.clone();
    let origin_harness_dir = pi
        .boxr_home()
        .join("sessions")
        .join(&parent)
        .join("harness");
    let origin_transcript = session_file_in(&origin_harness_dir, &origin_harness_id);
    let after_parent = fs::metadata(&origin_transcript)
        .expect("origin transcript")
        .len();

    let child_out = pi.run_with_fixture("resume", &["resume", &parent, "and now?"]);
    let child_stdout = stdout_of(&child_out);
    assert_eq!(
        child_out.status.code(),
        Some(0),
        "{}",
        stderr_of(&child_out)
    );
    let child = session_id_of(&child_stdout);
    let after_child = fs::metadata(&origin_transcript)
        .expect("origin transcript after first resume")
        .len();
    assert!(
        after_child > after_parent,
        "{after_child} <= {after_parent}"
    );

    let again = pi.run_with_fixture("resume", &["resume", &child, "and again?"]);
    let stdout = stdout_of(&again);
    assert_eq!(again.status.code(), Some(0), "{}", stderr_of(&again));
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(
        stdout.contains(&format!("resumedFrom: {child}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("harnessSessionId: {origin_harness_id}")),
        "{stdout}"
    );

    let args = pi.recorded_args();
    assert_eq!(arg_after(&args, "--session-id"), origin_harness_id);
    assert_eq!(
        arg_after(&args, "--session-dir"),
        origin_harness_dir.display().to_string()
    );
    assert_eq!(pi.recorded_prompt(), "and again?");

    let grandchild = session_id_of(&stdout);
    assert_ne!(grandchild, child);
    let summary = summary_of(&pi.boxr_home(), &grandchild);
    assert_eq!(summary["mode"], "resume");
    assert_eq!(summary["resumedFrom"], child.as_str());
    assert_eq!(summary["harnessSessionId"], origin_harness_id.as_str());
    assert_eq!(summary["steps"], 2);

    let after_grandchild = fs::metadata(&origin_transcript)
        .expect("origin transcript after second resume")
        .len();
    assert!(
        after_grandchild > after_child,
        "{after_grandchild} <= {after_child}"
    );

    let child_harness = pi.boxr_home().join("sessions").join(&child).join("harness");
    let child_files: Vec<_> = fs::read_dir(&child_harness)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        child_files.is_empty(),
        "second hop wrote under the continuation harness dir: {child_files:?}"
    );

    let lines = normalized_lines(
        &pi.boxr_home()
            .join("sessions")
            .join(&grandchild)
            .join("normalized.jsonl"),
    );
    assert_eq!(lines[0]["extra"]["mode"], "resume");
    assert_eq!(lines[0]["extra"]["resumedFrom"], child.as_str());
    let steps = steps_of(&lines);
    assert_eq!(sources_of(steps), ["user", "agent"]);
    assert_eq!(steps[1]["message"], "follow-up answered by boxr fixture");
}

fn session_file_in(dir: &std::path::Path, session_id: &str) -> PathBuf {
    let suffix = format!("_{session_id}.jsonl");
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()))
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        })
        .collect();
    files.sort();
    assert_eq!(files.len(), 1, "one session file in {}", dir.display());
    files.pop().expect("the session file")
}
