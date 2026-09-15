use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Harness {
    root: tempfile::TempDir,
    bin_dir: PathBuf,
    args_file: PathBuf,
    prompt_file: PathBuf,
    exit_code: String,
    withhold_transcript: bool,
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

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_path(args, Some(&self.bin_dir))
    }

    fn run_with_path(&self, args: &[&str], bin_dir: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxr"));
        command
            .args(args)
            .current_dir(self.root.path().join("work"))
            .env("BOXR_HOME", self.boxr_home())
            .env("CLAUDE_CONFIG_DIR", self.root.path().join("claude"))
            .env("BOXR_FAKE_CLAUDE_FIXTURE", fixture_dir())
            .env("BOXR_FAKE_CLAUDE_EXIT", &self.exit_code)
            .env("BOXR_FAKE_CLAUDE_DELAY_MS", "5")
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
        command.output().expect("boxr runs")
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/hello")
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
    let original = fs::read(fixture_dir().join("transcript.jsonl")).expect("fixture transcript");
    assert_eq!(copied, original);

    let stream = fs::read_to_string(raw.join("stream.jsonl")).expect("copied stream");
    let fixture_stream =
        fs::read_to_string(fixture_dir().join("stream.jsonl")).expect("fixture stream");
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
    assert!(!stdout.contains("Read the raw transcript"), "{stdout}");
    assert!(!session_dir(&harness).join("raw/transcript.jsonl").exists());
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
