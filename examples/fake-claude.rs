use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::thread::sleep;
use std::time::Duration;

const FIXTURE_ENV: &str = "BOXR_FAKE_CLAUDE_FIXTURE";
const EXIT_ENV: &str = "BOXR_FAKE_CLAUDE_EXIT";
const ARGS_ENV: &str = "BOXR_FAKE_CLAUDE_ARGS";
const PROMPT_ENV: &str = "BOXR_FAKE_CLAUDE_PROMPT";
const DELAY_ENV: &str = "BOXR_FAKE_CLAUDE_DELAY_MS";
const NO_TRANSCRIPT_ENV: &str = "BOXR_FAKE_CLAUDE_NO_TRANSCRIPT";
const HANG_AFTER_ENV: &str = "BOXR_FAKE_CLAUDE_HANG_AFTER";
const PID_ENV: &str = "BOXR_FAKE_CLAUDE_PID";
const CONFIG_OUT_ENV: &str = "BOXR_FAKE_CLAUDE_CONFIG_OUT";
const COMMIT_ENV: &str = "BOXR_FAKE_CLAUDE_COMMIT";
const GIT_ENV: &str = "BOXR_FAKE_CLAUDE_GIT";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Some(path) = env::var_os(ARGS_ENV) {
        if let Err(error) = fs::write(PathBuf::from(path), args.join("\n")) {
            eprintln!("cannot record arguments: {error}");
            return ExitCode::from(97);
        }
    }
    if args.first().map(String::as_str) == Some("auth") {
        return login();
    }

    let fixture = match env::var_os(FIXTURE_ENV) {
        Some(value) => PathBuf::from(value),
        None => {
            eprintln!("{FIXTURE_ENV} is not set");
            return ExitCode::from(97);
        }
    };
    let resumed = flag_value(&args, "--resume");

    let mut prompt = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut prompt) {
        eprintln!("cannot read the prompt from stdin: {error}");
        return ExitCode::from(97);
    }
    if prompt.is_empty() {
        eprintln!("Error: Input must be provided either through stdin or as a prompt argument when using --print");
        return ExitCode::from(1);
    }
    if let Some(path) = env::var_os(PROMPT_ENV) {
        if let Err(error) = fs::write(PathBuf::from(path), &prompt) {
            eprintln!("cannot record the prompt: {error}");
            return ExitCode::from(97);
        }
    }

    let stream = match fs::read_to_string(fixture.join("stream.jsonl")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read the fixture stream: {error}");
            return ExitCode::from(97);
        }
    };
    let transcript = match fs::read_to_string(fixture.join("transcript.jsonl")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read the fixture transcript: {error}");
            return ExitCode::from(97);
        }
    };

    let session_id = match session_id(&stream) {
        Some(id) => id,
        None => {
            eprintln!("the fixture stream has no session id");
            return ExitCode::from(97);
        }
    };

    let transcript_path = match transcript_path(&session_id) {
        Some(path) => path,
        None => {
            eprintln!("cannot locate a claude config directory");
            return ExitCode::from(97);
        }
    };
    if let Some(parent) = transcript_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!("cannot create the transcript directory: {error}");
            return ExitCode::from(97);
        }
    }

    let delay = delay();

    let mut transcript_file = if env::var_os(NO_TRANSCRIPT_ENV).is_some() {
        None
    } else {
        match open_transcript(&transcript_path, resumed.as_deref()) {
            Ok(file) => Some(file),
            Err(error) => {
                eprintln!("cannot open the transcript: {error}");
                return ExitCode::from(1);
            }
        }
    };

    if let Some(message) = env::var_os(COMMIT_ENV) {
        if let Err(error) = commit(&message) {
            eprintln!("cannot commit: {error}");
            return ExitCode::from(97);
        }
    }
    let hang_after = env::var(HANG_AFTER_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok());

    let mut stdout = std::io::stdout();
    let mut transcript_lines = transcript.lines();
    for (emitted, line) in stream.lines().enumerate() {
        if hang_after == Some(emitted) {
            if let Some(path) = env::var_os(PID_ENV) {
                let _ = fs::write(PathBuf::from(path), std::process::id().to_string());
            }
            sleep(Duration::from_secs(60));
            eprintln!("fake claude hung and was never killed");
            return ExitCode::from(97);
        }
        if let Some(entry) = transcript_lines.next() {
            append(&mut transcript_file, entry);
        }
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
        sleep(delay);
    }
    for entry in transcript_lines {
        append(&mut transcript_file, entry);
        sleep(delay);
    }

    exit_code()
}

fn commit(message: &std::ffi::OsStr) -> std::io::Result<()> {
    let git = git();
    run_git(&git, &["add", "-A"])?;
    let message = message.to_string_lossy();
    run_git(&git, &["commit", "-m", message.as_ref()])
}

fn git() -> PathBuf {
    env::var_os(GIT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("git"))
}

fn run_git(git: &PathBuf, args: &[&str]) -> std::io::Result<()> {
    let status = Command::new(git).args(args).status()?;
    if status.success() {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "git {} failed with {status}",
        args.join(" ")
    )))
}

fn login() -> ExitCode {
    let config_dir = match config_dir() {
        Some(dir) => dir,
        None => {
            eprintln!("cannot locate a claude config directory");
            return ExitCode::from(97);
        }
    };
    echo_config_dir(&config_dir);
    if let Err(error) = fs::create_dir_all(&config_dir) {
        eprintln!("cannot create the config directory: {error}");
        return ExitCode::from(97);
    }
    if let Err(error) = fs::write(
        config_dir.join("login.marker"),
        config_dir.display().to_string(),
    ) {
        eprintln!("cannot write the login marker: {error}");
        return ExitCode::from(97);
    }
    sleep(delay());

    exit_code()
}

fn echo_config_dir(config_dir: &std::path::Path) {
    if let Some(path) = env::var_os(CONFIG_OUT_ENV) {
        let _ = fs::write(PathBuf::from(path), config_dir.display().to_string());
    }
}

fn delay() -> Duration {
    Duration::from_millis(
        env::var(DELAY_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5),
    )
}

fn exit_code() -> ExitCode {
    let code: u8 = env::var(EXIT_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if code != 0 {
        eprintln!("fake claude failing on purpose with exit code {code}");
    }
    ExitCode::from(code)
}

fn append(transcript_file: &mut Option<fs::File>, entry: &str) {
    if let Some(file) = transcript_file {
        let _ = writeln!(file, "{entry}");
        let _ = file.flush();
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let position = args.iter().position(|arg| arg == flag)?;
    args.get(position + 1).cloned()
}

fn open_transcript(path: &PathBuf, resume: Option<&str>) -> std::io::Result<fs::File> {
    match resume {
        Some(id) if !path.is_file() => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no conversation found with session ID: {id}"),
        )),
        Some(_) => fs::OpenOptions::new().append(true).open(path),
        None => fs::File::create(path),
    }
}

fn session_id(stream: &str) -> Option<String> {
    let line = stream
        .lines()
        .find(|line| line.contains("\"session_id\""))?;
    let start = line.find("\"session_id\":\"")? + "\"session_id\":\"".len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn transcript_path(session_id: &str) -> Option<PathBuf> {
    let config_dir = config_dir()?;
    echo_config_dir(&config_dir);
    let cwd = env::current_dir().ok()?;
    let slug: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Some(
        config_dir
            .join("projects")
            .join(slug)
            .join(format!("{session_id}.jsonl")),
    )
}

fn config_dir() -> Option<PathBuf> {
    match env::var_os("CLAUDE_CONFIG_DIR") {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => {
            let dir = home_dir()?.join(".claude");
            echo_config_dir(&dir);
            Some(dir)
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    let keys: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME"]
    };
    keys.iter()
        .filter_map(env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}
