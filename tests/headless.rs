use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant};

struct Harness {
    root: tempfile::TempDir,
    bin_dir: PathBuf,
    args_file: PathBuf,
    prompt_file: PathBuf,
    exit_code: String,
    withhold_transcript: bool,
    delay_ms: String,
    kill_after: Option<String>,
    fixture: &'static str,
}

impl Harness {
    fn new() -> Harness {
        let root = tempfile::tempdir().expect("temp dir");
        let bin_dir = root.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("bin dir");
        fs::create_dir_all(root.path().join("work")).expect("work dir");
        fs::create_dir_all(root.path().join("claude")).expect("claude config dir");
        fs::copy(fake_claude(), bin_dir.join(fake_name())).expect("install fake claude");
        let prompt_file = root.path().join("claude-prompt.txt");
        Harness {
            root,
            bin_dir,
            args_file: PathBuf::new(),
            prompt_file,
            exit_code: "0".to_string(),
            withhold_transcript: false,
            delay_ms: "5".to_string(),
            kill_after: None,
            fixture: "hello",
        }
    }

    fn boxr_home(&self) -> PathBuf {
        self.root.path().join("boxr")
    }

    fn write_config(&self, body: &str) {
        let home = self.boxr_home();
        fs::create_dir_all(&home).expect("boxr home");
        fs::write(home.join("config.json"), body).expect("config");
    }

    fn record_args(&mut self) {
        self.args_file = self.root.path().join("claude-args.txt");
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

    #[cfg(windows)]
    fn install_cmd_shim(&self) {
        fs::remove_file(self.bin_dir.join(fake_name())).expect("remove fake claude.exe");
        let tools = self.root.path().join("tools");
        fs::create_dir_all(&tools).expect("tools dir");
        let fake = tools.join("fake-claude.exe");
        fs::copy(fake_claude(), &fake).expect("install fake claude outside PATH");
        fs::write(
            self.bin_dir.join("claude.cmd"),
            format!("@\"{}\" %*\r\n", fake.display()),
        )
        .expect("claude.cmd shim");
    }

    fn fail_with(&mut self, code: &str) {
        self.exit_code = code.to_string();
    }

    fn withhold_transcript(&mut self) {
        self.withhold_transcript = true;
    }

    fn slow_harness(&mut self, delay_ms: u64) {
        self.delay_ms = delay_ms.to_string();
    }

    fn kill_after(&mut self, lines: usize) {
        self.kill_after = Some(lines.to_string());
    }

    fn use_fixture(&mut self, name: &'static str) {
        self.fixture = name;
    }

    fn harness_transcript_lines(&self) -> usize {
        let projects = self.root.path().join("claude").join("projects");
        fs::read_dir(projects)
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|project| fs::read_dir(project.path()).into_iter().flatten().flatten())
            .filter_map(|file| fs::read_to_string(file.path()).ok())
            .map(|text| text.lines().count())
            .sum()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_path(args, Some(&self.bin_dir))
    }

    fn spawn(&self, args: &[&str]) -> Child {
        self.command(args, Some(&self.bin_dir))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("boxr starts")
    }

    fn run_with_path(&self, args: &[&str], bin_dir: Option<&Path>) -> Output {
        self.command(args, bin_dir).output().expect("boxr runs")
    }

    fn command(&self, args: &[&str], bin_dir: Option<&Path>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxr"));
        command
            .args(args)
            .current_dir(self.root.path().join("work"))
            .env("BOXR_HOME", self.boxr_home())
            .env("CLAUDE_CONFIG_DIR", self.root.path().join("claude"))
            .env("BOXR_FAKE_CLAUDE_FIXTURE", fixture_dir(self.fixture))
            .env("BOXR_FAKE_CLAUDE_EXIT", &self.exit_code)
            .env("BOXR_FAKE_CLAUDE_DELAY_MS", &self.delay_ms)
            .env("BOXR_FAKE_CLAUDE_PROMPT", &self.prompt_file)
            .env(
                "PATH",
                bin_dir
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.root.path().join("empty")),
            );
        if !self.args_file.as_os_str().is_empty() {
            command.env("BOXR_FAKE_CLAUDE_ARGS", &self.args_file);
        }
        if self.withhold_transcript {
            command.env("BOXR_FAKE_CLAUDE_NO_TRANSCRIPT", "1");
        }
        if let Some(lines) = &self.kill_after {
            command.env("BOXR_FAKE_CLAUDE_KILL_AFTER", lines);
        }
        command
    }
}

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude")
        .join(name)
}

fn fake_name() -> &'static str {
    if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    }
}

fn fake_claude() -> PathBuf {
    let boxr = Path::new(env!("CARGO_BIN_EXE_boxr"));
    let path = boxr
        .parent()
        .expect("target directory")
        .join("examples")
        .join(if cfg!(windows) {
            "fake-claude.exe"
        } else {
            "fake-claude"
        });
    assert!(
        path.is_file(),
        "the fake claude example is missing at {}; build it with `cargo build --example fake-claude`",
        path.display()
    );
    path
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn session_dir(harness: &Harness) -> PathBuf {
    let sessions = harness.boxr_home().join("sessions");
    let mut entries: Vec<PathBuf> = fs::read_dir(&sessions)
        .expect("sessions dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    entries.pop().expect("one session directory")
}

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
    assert!(stdout.contains("help[2]:"), "{stdout}");

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

fn session_id_of(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("id: "))
        .expect("session id line")
        .to_string()
}

fn normalized_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("normalized ledger")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("a json line"))
        .collect()
}

fn summary_of(harness: &Harness, id: &str) -> Value {
    fs::read_to_string(harness.boxr_home().join("summary.jsonl"))
        .expect("summary ledger")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("a json line"))
        .find(|value| value["id"] == id)
        .expect("a summary line for the session")
}

fn assert_valid_steps(steps: &[Value]) {
    for (index, step) in steps.iter().enumerate() {
        let none = Vec::new();
        assert_eq!(step["step_id"], (index + 1) as i64, "{step}");
        let source = step["source"].as_str().expect("step source");
        assert!(
            matches!(source, "system" | "user" | "agent"),
            "unknown source {source}"
        );
        assert!(step["message"].is_string(), "{step}");
        if let Some(timestamp) = step["timestamp"].as_str() {
            assert!(
                timestamp.ends_with('Z') && timestamp.contains('T'),
                "{step}"
            );
        }
        if source != "agent" {
            for field in [
                "model_name",
                "reasoning_effort",
                "reasoning_content",
                "tool_calls",
                "observation",
                "metrics",
            ] {
                assert!(step.get(field).is_none(), "{field} on a {source} step");
            }
        }
        let mut call_ids = Vec::new();
        for call in step["tool_calls"].as_array().unwrap_or(&none) {
            call_ids.push(call["tool_call_id"].as_str().expect("tool_call_id"));
            assert!(call["function_name"].is_string(), "{call}");
            assert!(call["arguments"].is_object(), "{call}");
        }
        for result in step["observation"]["results"].as_array().unwrap_or(&none) {
            assert!(result["content"].is_string(), "{result}");
            if let Some(source_call_id) = result["source_call_id"].as_str() {
                assert!(
                    call_ids.contains(&source_call_id),
                    "observation {source_call_id} has no tool call on step {step}"
                );
            }
        }
    }
}

fn assert_valid_atif(document: &Value) {
    assert_eq!(document["schema_version"], "ATIF-v1.8");
    assert!(document["session_id"].is_string(), "{document}");
    assert!(document["agent"]["name"].is_string(), "{document}");
    assert!(document["agent"]["version"].is_string(), "{document}");
    let steps = document["steps"].as_array().expect("steps array");
    assert!(!steps.is_empty(), "{document}");
    assert_valid_steps(steps);
    let metrics = &document["final_metrics"];
    assert_eq!(metrics["total_steps"], steps.len() as i64, "{document}");
    assert!(metrics["total_prompt_tokens"].is_u64(), "{document}");
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
fn a_summary_line_records_the_session_with_its_token_counts() {
    let harness = Harness::new();
    let output = harness.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness, &session_id_of(&stdout));
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
    assert!(stdout.contains("help[1]:"), "{stdout}");
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
    let sources: Vec<&str> = steps
        .iter()
        .map(|step| step["source"].as_str().expect("source"))
        .collect();
    assert_eq!(
        sources,
        ["user", "agent", "agent", "agent", "agent", "agent"]
    );
    for (step, call_id, content) in [
        (
            &steps[2],
            "toolu_fixture0000000001",
            "default workflow live run",
        ),
        (
            &steps[3],
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
    assert!(steps[5]["message"]
        .as_str()
        .expect("message")
        .contains("REVIEW-VERDICT: clean"));
}

#[test]
fn a_reply_split_across_transcript_lines_counts_its_tokens_once() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    let output = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    let summary = summary_of(&harness, &session_id_of(&stdout));
    assert_eq!(summary["steps"], 6);
    assert_eq!(summary["promptTokens"], 25240 + 28999);
    assert_eq!(summary["completionTokens"], 408 + 507);
    assert_eq!(summary["cachedTokens"], 10023 + 25238);

    let lines = normalized_lines(&session_dir(&harness).join("normalized.jsonl"));
    let closing = &lines.last().expect("closing line")["final_metrics"];
    assert_eq!(closing["total_prompt_tokens"], 25240 + 28999);
    assert_eq!(closing["total_completion_tokens"], 408 + 507);
    let with_metrics = lines
        .iter()
        .filter(|line| line["metrics"].is_object())
        .count();
    assert_eq!(with_metrics, 2, "{lines:?}");
}

#[test]
fn a_harness_killed_mid_run_still_leaves_a_ledger_and_an_interrupted_summary() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.kill_after(9);
    let output = harness.run(&["--harness", "claude", "--model", "opus", "review"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("status: interrupted"), "{stdout}");

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

    let summary = summary_of(&harness, &session_id_of(&stdout));
    assert_eq!(summary["status"], "interrupted");
    assert_eq!(summary["exitCode"], 137);
}
