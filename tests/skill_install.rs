use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Sandbox {
        let root = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(root.path().join("bin")).expect("bin dir");
        Sandbox { root }
    }

    fn boxr_home(&self) -> PathBuf {
        self.root.path().join("boxr")
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.root.path().join(name);
        fs::create_dir_all(&path).expect("dir");
        path
    }

    fn skill_at(&self, config_dir: &str) -> PathBuf {
        self.root
            .path()
            .join(config_dir)
            .join("skills/boxr-prompts/SKILL.md")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxr"));
        command
            .args(args)
            .env("BOXR_HOME", self.boxr_home())
            .env("HOME", self.root.path())
            .env("USERPROFILE", self.root.path())
            .env("PATH", self.root.path().join("bin"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("PI_CODING_AGENT_DIR");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("boxr runs")
    }
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn skill_body(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

#[test]
fn claude_installs_into_the_user_skill_directory_under_the_test_home() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["skill", "install", "--harness", "claude"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("skills[1]:\n  boxr-prompts\n"), "{stdout}");
    assert!(stdout.contains("action: install"), "{stdout}");
    assert!(stdout.contains("harnesses: 1"), "{stdout}");
    assert!(stdout.contains("claude: "), "{stdout}");

    let skill = sandbox.skill_at(".claude");
    assert!(skill.is_file(), "missing {}", skill.display());
    assert!(skill_body(&skill).contains("name: boxr-prompts"));
}

#[test]
fn a_config_dir_override_moves_the_claude_skill_directory() {
    let sandbox = Sandbox::new();
    let config = sandbox.dir("claude-config");
    let output = sandbox
        .command(&["skill", "install", "--harness", "claude"])
        .env("CLAUDE_CONFIG_DIR", &config)
        .output()
        .expect("boxr runs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(config.join("skills/boxr-prompts/SKILL.md").is_file());
    assert!(!sandbox.skill_at(".claude").exists());
}

#[test]
fn all_installs_into_every_supported_harness() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["skill", "install"]);
    let stdout = stdout_of(&output);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("harnesses: 3"), "{stdout}");
    assert!(stdout.contains("claude: "), "{stdout}");
    assert!(stdout.contains("codex: "), "{stdout}");
    assert!(stdout.contains("pi: "), "{stdout}");

    for config_dir in [".claude", ".codex", ".pi/agent"] {
        let skill = sandbox.skill_at(config_dir);
        assert!(skill.is_file(), "missing {}", skill.display());
        assert!(skill_body(&skill).contains("name: boxr-prompts"));
    }
}

#[test]
fn codex_and_pi_honour_their_config_dir_overrides() {
    let sandbox = Sandbox::new();
    let codex = sandbox.dir("codex-home");
    let pi = sandbox.dir("pi-agent");
    let output = sandbox
        .command(&["skill", "install"])
        .env("CODEX_HOME", &codex)
        .env("PI_CODING_AGENT_DIR", &pi)
        .output()
        .expect("boxr runs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(codex.join("skills/boxr-prompts/SKILL.md").is_file());
    assert!(pi.join("skills/boxr-prompts/SKILL.md").is_file());
    assert!(!sandbox.skill_at(".codex").exists());
    assert!(!sandbox.skill_at(".pi/agent").exists());
}

#[test]
fn reinstalling_replaces_the_previous_version_without_duplicates() {
    let sandbox = Sandbox::new();
    sandbox.run(&["skill", "install", "--harness", "claude"]);
    let skills = sandbox.root.path().join(".claude/skills");
    fs::write(
        skills.join("boxr-prompts/stale.md"),
        "from an older install",
    )
    .expect("stale file");

    let output = sandbox.run(&["skill", "install", "--harness", "claude"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let installed: Vec<String> = fs::read_dir(&skills)
        .expect("skills dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(installed.len(), 1, "{installed:?}");

    let contents: Vec<String> = fs::read_dir(skills.join("boxr-prompts"))
        .expect("skill dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(contents, vec!["SKILL.md".to_string()], "{contents:?}");
    assert!(skill_body(&skills.join("boxr-prompts/SKILL.md")).contains("name: boxr-prompts"));
}

#[test]
fn an_unknown_harness_is_a_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["skill", "install", "--harness", "gemini"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("unknown harness"), "{stderr}");
    assert!(stderr.contains("all"), "{stderr}");
    assert!(!sandbox.root.path().join(".claude").exists());
}
