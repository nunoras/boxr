mod common;

use common::install_fake;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIXTURE_SESSION_ID: &str = "11111111-2222-4333-8444-555555555555";

fn toon_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}

struct Sandbox {
    root: tempfile::TempDir,
    bin_dir: PathBuf,
    work: PathBuf,
    user_home: PathBuf,
    config_out: PathBuf,
    exit_code: String,
    delay_ms: String,
    withhold_transcript: bool,
}

impl Sandbox {
    fn new() -> Sandbox {
        let root = tempfile::tempdir().expect("temp dir");
        let bin_dir = root.path().join("bin");
        let work = root.path().join("work");
        let user_home = root.path().join("user");
        for dir in [&bin_dir, &work, &user_home] {
            fs::create_dir_all(dir).expect("dir");
        }
        install_fake(&fake_claude(), &bin_dir.join(fake_name()));
        Sandbox {
            config_out: root.path().join("seen-config-dir.txt"),
            root,
            bin_dir,
            work,
            user_home,
            exit_code: "0".to_string(),
            delay_ms: "5".to_string(),
            withhold_transcript: false,
        }
    }

    fn boxr_home(&self) -> PathBuf {
        self.root.path().join("boxr")
    }

    fn account_dir(&self, name: &str) -> PathBuf {
        self.boxr_home().join("accounts").join("claude").join(name)
    }

    fn write_config(&self, body: &str) {
        let home = self.boxr_home();
        fs::create_dir_all(&home).expect("boxr home");
        fs::write(home.join("config.json"), body).expect("config");
    }

    fn command(&self, args: &[&str], config_out: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxr"));
        command
            .args(args)
            .current_dir(&self.work)
            .env("BOXR_HOME", self.boxr_home())
            .env("HOME", &self.user_home)
            .env("USERPROFILE", &self.user_home)
            .env("PATH", &self.bin_dir)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("BOXR_FAKE_CLAUDE_FIXTURE", fixture_dir())
            .env("BOXR_FAKE_CLAUDE_EXIT", &self.exit_code)
            .env("BOXR_FAKE_CLAUDE_DELAY_MS", &self.delay_ms)
            .env(
                "BOXR_FAKE_CLAUDE_PROMPT",
                self.root.path().join("prompt.txt"),
            )
            .env("BOXR_FAKE_CLAUDE_CONFIG_OUT", config_out);
        if self.withhold_transcript {
            command.env("BOXR_FAKE_CLAUDE_NO_TRANSCRIPT", "1");
        }
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args, &self.config_out)
            .output()
            .expect("boxr runs")
    }

    fn seen_config_dir(&self) -> String {
        fs::read_to_string(&self.config_out).expect("the fake claude recorded the config dir")
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

fn add_profile(sandbox: &Sandbox, name: &str) {
    let output = sandbox.run(&["account", "add", "--harness", "claude", "--name", name]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
}

#[test]
fn account_add_creates_an_isolated_profile_and_runs_the_login() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["account", "add", "--harness", "claude", "--name", "work"]);
    let stdout = stdout_of(&output);
    let dir = sandbox.account_dir("work");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("harness: claude"), "{stdout}");
    assert!(stdout.contains("name: work"), "{stdout}");
    assert!(stdout.contains("status: created"), "{stdout}");
    assert!(stdout.contains("dir: "), "{stdout}");
    assert!(stdout.contains(&toon_path(&dir)), "{stdout}");
    assert!(stdout.contains("help[2]:"), "{stdout}");
    assert!(stdout.contains("boxr account list"), "{stdout}");

    assert!(dir.is_dir(), "the profile directory was created");
    assert!(dir.join("login.marker").is_file(), "the login ran");
    assert_eq!(sandbox.seen_config_dir(), dir.display().to_string());
    assert!(
        !sandbox.user_home.join(".claude").exists(),
        "the user's own claude config was left alone"
    );
}

#[cfg(unix)]
#[test]
fn an_account_directory_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    let mode = fs::metadata(sandbox.account_dir("work"))
        .expect("profile metadata")
        .permissions()
        .mode();

    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn adding_an_existing_profile_runs_the_login_again() {
    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    let output = sandbox.run(&["account", "add", "--harness", "claude", "--name", "work"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("status: updated"), "{stdout}");
    assert!(sandbox.account_dir("work").join("login.marker").is_file());
}

#[test]
fn a_failed_login_is_a_clear_error_and_leaves_no_profile() {
    let mut sandbox = Sandbox::new();
    sandbox.exit_code = "4".to_string();
    let output = sandbox.run(&["account", "add", "--harness", "claude", "--name", "work"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("login for account `work` failed with exit code 4"),
        "{stderr}"
    );
    assert!(stderr.contains("help[1]:"), "{stderr}");
    assert!(!sandbox.account_dir("work").exists(), "no profile was left");

    let listed = stdout_of(&sandbox.run(&["account", "list"]));
    assert!(
        listed.contains("accounts[0]{harness,name,dir}:"),
        "{listed}"
    );
}

#[test]
fn a_failed_login_on_an_existing_profile_keeps_it() {
    let mut sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    sandbox.exit_code = "4".to_string();
    let output = sandbox.run(&["account", "add", "--harness", "claude", "--name", "work"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("help[2]:"), "{stderr}");
    assert!(stderr.contains("boxr account remove"), "{stderr}");
    assert!(
        sandbox.account_dir("work").join("login.marker").is_file(),
        "the existing profile was kept"
    );
}

#[cfg(unix)]
#[test]
fn a_login_cancelled_with_ctrl_c_leaves_no_profile() {
    use std::os::unix::process::CommandExt;

    let mut sandbox = Sandbox::new();
    sandbox.delay_ms = "10000".to_string();
    let mut child = sandbox
        .command(
            &["account", "add", "--harness", "claude", "--name", "work"],
            &sandbox.config_out,
        )
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("boxr starts");

    let marker = sandbox.account_dir("work").join("login.marker");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !marker.is_file() {
        assert!(
            std::time::Instant::now() < deadline,
            "the login never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let interrupted = Command::new("kill")
        .args(["-s", "INT", "--", &format!("-{}", child.id())])
        .status()
        .expect("kill runs");
    assert!(interrupted.success());

    let status = child.wait().expect("boxr exits");
    assert_eq!(status.code(), Some(1), "{status:?}");
    assert!(!sandbox.account_dir("work").exists(), "no profile was left");
}

#[test]
fn account_list_prints_profiles_as_toon() {
    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    add_profile(&sandbox, "personal");
    let output = sandbox.run(&["account", "list"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stdout.contains("accounts[2]{harness,name,dir}:"),
        "{stdout}"
    );
    assert!(stdout.contains("  claude,work,"), "{stdout}");
    assert!(stdout.contains("  claude,personal,"), "{stdout}");
    assert!(
        stdout.contains(&toon_path(&sandbox.account_dir("work"))),
        "{stdout}"
    );
    assert!(stdout.contains("help[2]:"), "{stdout}");
}

#[test]
fn account_list_with_no_profiles_is_empty_and_points_at_add() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["account", "list"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stdout.contains("accounts[0]{harness,name,dir}:"),
        "{stdout}"
    );
    assert!(
        stdout.contains("boxr account add --harness claude --name work"),
        "{stdout}"
    );
}

#[test]
fn account_remove_needs_confirmation_and_then_deletes() {
    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");

    let refused = sandbox.run(&["account", "remove", "--harness", "claude", "--name", "work"]);
    let stderr = stderr_of(&refused);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("needs confirmation"), "{stderr}");
    assert!(stderr.contains("--yes"), "{stderr}");
    assert!(sandbox.account_dir("work").is_dir(), "the profile survived");

    let removed = sandbox.run(&[
        "account",
        "remove",
        "--harness",
        "claude",
        "--name",
        "work",
        "--yes",
    ]);
    let stdout = stdout_of(&removed);
    assert_eq!(
        removed.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&removed)
    );
    assert!(stdout.contains("status: removed"), "{stdout}");
    assert!(stdout.contains("help[1]:"), "{stdout}");
    assert!(!sandbox.account_dir("work").exists(), "the profile is gone");
}

#[test]
fn account_remove_of_an_unknown_profile_points_at_list() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "account",
        "remove",
        "--harness",
        "claude",
        "--name",
        "nope",
        "--yes",
    ]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("unknown account `nope` for harness `claude`"),
        "{stderr}"
    );
    assert!(stderr.contains("boxr account list"), "{stderr}");
}

#[test]
fn account_remove_with_an_unknown_harness_deletes_nothing() {
    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    for (harness, name) in [("..", "accounts"), ("", "claude"), ("../..", "boxr")] {
        let output = sandbox.run(&[
            "account",
            "remove",
            "--harness",
            harness,
            "--name",
            name,
            "--yes",
        ]);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "{harness}: {stderr}");
        assert!(stderr.contains("unknown harness"), "{harness}: {stderr}");
    }
    assert!(sandbox.account_dir("work").is_dir(), "the profile survived");
}

#[test]
fn an_invalid_account_name_is_a_usage_error() {
    let sandbox = Sandbox::new();
    for name in ["a/b", "..", "with space"] {
        let output = sandbox.run(&["account", "add", "--harness", "claude", "--name", name]);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "{name}: {stderr}");
        assert!(stderr.contains("invalid account name"), "{name}: {stderr}");
    }
    assert!(!sandbox.boxr_home().join("accounts").exists());
}

#[test]
fn an_unknown_harness_on_account_add_is_a_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["account", "add", "--harness", "gemini", "--name", "work"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unknown harness"), "{stderr}");
}

#[test]
fn a_launch_points_claude_at_the_profile_directory() {
    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    let output = sandbox.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--account",
        "work",
        "hello",
    ]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("status: ok"), "{stdout}");
    assert!(stdout.contains("account: work"), "{stdout}");
    assert!(stdout.contains("ledger: recorded"), "{stdout}");
    assert_eq!(
        sandbox.seen_config_dir(),
        sandbox.account_dir("work").display().to_string()
    );

    let copied = fs::read(sandbox.session_dir().join("raw/transcript.jsonl"))
        .expect("the transcript came from the profile directory");
    let original = fs::read(fixture_dir().join("transcript.jsonl")).expect("fixture transcript");
    assert_eq!(copied, original);
    assert!(
        !sandbox.user_home.join(".claude").exists(),
        "the user's own claude config was left alone"
    );
}

#[test]
fn a_launch_without_an_account_leaves_account_at_the_harness_default() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["--harness", "claude", "--model", "sonnet", "hello"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("account: harness-default"), "{stdout}");
}

#[test]
fn the_configured_default_account_is_used_when_the_flag_is_omitted() {
    let sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    sandbox.write_config(r#"{"defaults":{"harness":"claude","model":"sonnet","account":"work"}}"#);
    let output = sandbox.run(&["hello there"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("account: work"), "{stdout}");
    assert_eq!(
        sandbox.seen_config_dir(),
        sandbox.account_dir("work").display().to_string()
    );
}

#[test]
fn an_unknown_account_on_launch_is_a_clear_error_with_help() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--account",
        "nope",
        "hello",
    ]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("unknown account `nope` for harness `claude`"),
        "{stderr}"
    );
    assert!(stderr.contains("boxr account list"), "{stderr}");
    assert!(!sandbox.boxr_home().join("sessions").exists());
}

#[test]
fn two_launches_on_different_profiles_each_see_their_own_directory() {
    let mut sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    add_profile(&sandbox, "personal");
    sandbox.delay_ms = "400".to_string();

    let work_out = sandbox.root.path().join("work-config-dir.txt");
    let personal_out = sandbox.root.path().join("personal-config-dir.txt");
    let first = sandbox
        .command(
            &[
                "--harness",
                "claude",
                "--model",
                "sonnet",
                "--account",
                "work",
                "hello",
            ],
            &work_out,
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("work launch starts");
    let second = sandbox
        .command(
            &[
                "--harness",
                "claude",
                "--model",
                "sonnet",
                "--account",
                "personal",
                "hello",
            ],
            &personal_out,
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("personal launch starts");

    let first = first.wait_with_output().expect("work launch exits");
    let second = second.wait_with_output().expect("personal launch exits");

    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&first)
    );
    assert_eq!(
        second.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&second)
    );
    assert_eq!(
        fs::read_to_string(&work_out).expect("work config dir"),
        sandbox.account_dir("work").display().to_string()
    );
    assert_eq!(
        fs::read_to_string(&personal_out).expect("personal config dir"),
        sandbox.account_dir("personal").display().to_string()
    );
}

#[test]
fn a_profile_launch_never_reads_the_users_normal_claude_config() {
    let mut sandbox = Sandbox::new();
    add_profile(&sandbox, "work");
    let planted_dir = sandbox
        .user_home
        .join(".claude")
        .join("projects")
        .join("planted");
    fs::create_dir_all(&planted_dir).expect("planted project dir");
    fs::write(
        planted_dir.join(format!("{FIXTURE_SESSION_ID}.jsonl")),
        "planted user transcript\n",
    )
    .expect("planted transcript");

    sandbox.withhold_transcript = true;
    let output = sandbox.run(&[
        "--harness",
        "claude",
        "--model",
        "sonnet",
        "--account",
        "work",
        "hello",
    ]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(5), "{stdout}");
    assert!(stdout.contains("ledger: failed"), "{stdout}");
    assert!(!stdout.contains("planted"), "{stdout}");
    assert!(!sandbox.session_dir().join("raw/transcript.jsonl").exists());

    let entries: Vec<_> = fs::read_dir(sandbox.user_home.join(".claude"))
        .expect("user claude config")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(entries, vec![std::ffi::OsString::from("projects")]);
}
