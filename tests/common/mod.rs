#![allow(dead_code)]

use serde_json::Value;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub struct Harness {
    pub root: tempfile::TempDir,
    bin_dir: PathBuf,
    args_file: PathBuf,
    prompt_file: PathBuf,
    exit_code: String,
    withhold_transcript: bool,
    delay_ms: String,
    hang_after: Option<String>,
    fixture: &'static str,
    commit: Option<String>,
}

impl Harness {
    pub fn new() -> Harness {
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
            hang_after: None,
            fixture: "hello",
            commit: None,
        }
    }

    pub fn boxr_home(&self) -> PathBuf {
        self.root.path().join("boxr")
    }

    pub fn write_config(&self, body: &str) {
        let home = self.boxr_home();
        fs::create_dir_all(&home).expect("boxr home");
        fs::write(home.join("config.json"), body).expect("config");
    }

    pub fn record_args(&mut self) {
        self.args_file = self.root.path().join("claude-args.txt");
    }

    pub fn recorded_args(&self) -> Vec<String> {
        fs::read_to_string(&self.args_file)
            .expect("recorded args")
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn recorded_prompt(&self) -> String {
        fs::read_to_string(&self.prompt_file).expect("recorded prompt")
    }

    #[cfg(windows)]
    pub fn install_cmd_shim(&self) {
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

    pub fn fail_with(&mut self, code: &str) {
        self.exit_code = code.to_string();
    }

    pub fn withhold_transcript(&mut self) {
        self.withhold_transcript = true;
    }

    pub fn slow_harness(&mut self, delay_ms: u64) {
        self.delay_ms = delay_ms.to_string();
    }

    pub fn hang_after(&mut self, lines: usize) {
        self.hang_after = Some(lines.to_string());
    }

    pub fn pid_file(&self) -> PathBuf {
        self.root.path().join("claude.pid")
    }

    pub fn await_hung_harness(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let recorded = fs::read_to_string(self.pid_file()).unwrap_or_default();
            if recorded.parse::<u32>().is_ok() {
                return recorded;
            }
            assert!(Instant::now() < deadline, "the fake harness never hung");
            sleep(Duration::from_millis(20));
        }
    }

    pub fn harness_pid(&self) -> u32 {
        self.await_hung_harness()
            .parse()
            .expect("the fake harness pid")
    }

    pub fn kill_hung_harness(&self) {
        let pid = self.await_hung_harness();
        let status = if cfg!(windows) {
            Command::new("taskkill").args(["/F", "/PID", &pid]).status()
        } else {
            Command::new("kill").args(["-KILL", &pid]).status()
        }
        .expect("kill runs");
        assert!(status.success(), "could not kill the fake harness {pid}");
    }

    pub fn use_fixture(&mut self, name: &'static str) {
        self.fixture = name;
    }

    pub fn commit_with(&mut self, message: &str) {
        self.commit = Some(message.to_string());
    }

    pub fn work_dir(&self) -> PathBuf {
        self.root.path().join("work")
    }

    pub fn git(&self, args: &[&str]) -> Output {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.work_dir())
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {} failed: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    pub fn git_line(&self, args: &[&str]) -> String {
        let output = self.git(args);
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    pub fn git_init(&self) {
        self.git(&["init", "-q", "-b", "main"]);
        self.git(&["config", "user.name", "boxr test"]);
        self.git(&["config", "user.email", "boxr@example.com"]);
        self.git(&["config", "commit.gpgsign", "false"]);
    }

    pub fn harness_transcript_lines(&self) -> usize {
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

    fn search_path(&self, bin_dir: Option<&Path>) -> OsString {
        let Some(bin_dir) = bin_dir else {
            return self.root.path().join("empty").into_os_string();
        };
        let parent = std::env::var_os("PATH").unwrap_or_default();
        env::join_paths(std::iter::once(bin_dir.to_path_buf()).chain(env::split_paths(&parent)))
            .expect("joining PATH")
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.run_with_path(args, Some(&self.bin_dir))
    }

    pub fn spawn(&self, args: &[&str]) -> Child {
        let mut command = self.command(args, Some(&self.bin_dir));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            unsafe { console::AllocConsole() };
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        spawn_piped(&mut command)
    }

    #[cfg(windows)]
    pub fn spawn_in_own_console(&self, args: &[&str]) -> Child {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let mut command = self.command(args, Some(&self.bin_dir));
        command.creation_flags(CREATE_NEW_CONSOLE);
        spawn_piped(&mut command)
    }

    pub fn run_with_path(&self, args: &[&str], bin_dir: Option<&Path>) -> Output {
        self.command(args, bin_dir).output().expect("boxr runs")
    }

    pub fn run_with_fixture(&self, fixture: &'static str, args: &[&str]) -> Output {
        let mut command = self.command(args, Some(&self.bin_dir));
        command.env("BOXR_FAKE_CLAUDE_FIXTURE", fixture_dir(fixture));
        command.output().expect("boxr runs")
    }

    pub fn command(&self, args: &[&str], bin_dir: Option<&Path>) -> Command {
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
            .env("PATH", self.search_path(bin_dir));
        if !self.args_file.as_os_str().is_empty() {
            command.env("BOXR_FAKE_CLAUDE_ARGS", &self.args_file);
        }
        if self.withhold_transcript {
            command.env("BOXR_FAKE_CLAUDE_NO_TRANSCRIPT", "1");
        }
        if let Some(lines) = &self.hang_after {
            command
                .env("BOXR_FAKE_CLAUDE_HANG_AFTER", lines)
                .env("BOXR_FAKE_CLAUDE_PID", self.pid_file());
        }
        if let Some(message) = &self.commit {
            command
                .env("BOXR_FAKE_CLAUDE_COMMIT", message)
                .env("BOXR_FAKE_CLAUDE_GIT", git_exe());
        }
        command
    }
}

#[cfg(windows)]
pub mod console {
    #[link(name = "kernel32")]
    extern "system" {
        pub fn AllocConsole() -> i32;
        pub fn GenerateConsoleCtrlEvent(event: u32, process_group: u32) -> i32;
    }
}

pub fn spawn_piped(command: &mut Command) -> Child {
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("boxr starts")
}

pub fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude")
        .join(name)
}

pub fn fake_name() -> &'static str {
    if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    }
}

pub fn fake_claude() -> PathBuf {
    example_binary("fake-claude")
}

pub fn git_exe() -> PathBuf {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let path = std::env::var_os("PATH").expect("PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .expect("git on PATH")
}

pub fn example_binary(name: &str) -> PathBuf {
    let boxr = Path::new(env!("CARGO_BIN_EXE_boxr"));
    let path = boxr
        .parent()
        .expect("target directory")
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "the {name} example is missing at {}; build it with `cargo build --example {name}`",
        path.display()
    );
    path
}

pub fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub fn session_dir(harness: &Harness) -> PathBuf {
    let sessions = harness.boxr_home().join("sessions");
    let mut entries: Vec<PathBuf> = fs::read_dir(&sessions)
        .expect("sessions dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    entries.pop().expect("one session directory")
}

pub fn session_id_of(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("id: "))
        .expect("session id line")
        .to_string()
}

pub fn field_of(stdout: &str, name: &str) -> String {
    let prefix = format!("{name}: ");
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("no `{name}` field in:\n{stdout}"))
        .to_string()
}

pub fn normalized_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("normalized ledger")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("a json line"))
        .collect()
}

pub fn summary_of(home: &Path, id: &str) -> Value {
    fs::read_to_string(home.join("summary.jsonl"))
        .expect("summary ledger")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("a json line"))
        .find(|value| value["id"] == id)
        .expect("a summary line for the session")
}

pub fn sources_of(steps: &[Value]) -> Vec<&str> {
    steps
        .iter()
        .map(|step| step["source"].as_str().expect("step source"))
        .collect()
}

pub fn assert_valid_steps(steps: &[Value]) {
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

pub fn assert_valid_atif(document: &Value) {
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
