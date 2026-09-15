use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

const FIXTURE_ENV: &str = "BOXR_FAKE_CLAUDE_FIXTURE";
const EXIT_ENV: &str = "BOXR_FAKE_CLAUDE_EXIT";
const ARGS_ENV: &str = "BOXR_FAKE_CLAUDE_ARGS";
const DELAY_ENV: &str = "BOXR_FAKE_CLAUDE_DELAY_MS";

fn main() -> ExitCode {
    let fixture = match env::var_os(FIXTURE_ENV) {
        Some(value) => PathBuf::from(value),
        None => {
            eprintln!("{FIXTURE_ENV} is not set");
            return ExitCode::from(97);
        }
    };
    if let Some(path) = env::var_os(ARGS_ENV) {
        let args: Vec<String> = env::args().skip(1).collect();
        if let Err(error) = fs::write(PathBuf::from(path), args.join("\n")) {
            eprintln!("cannot record arguments: {error}");
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

    let delay = Duration::from_millis(
        env::var(DELAY_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5),
    );

    let mut transcript_file = match fs::File::create(&transcript_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("cannot create the transcript: {error}");
            return ExitCode::from(97);
        }
    };

    let mut stdout = std::io::stdout();
    let mut transcript_lines = transcript.lines();
    for line in stream.lines() {
        if let Some(entry) = transcript_lines.next() {
            let _ = writeln!(transcript_file, "{entry}");
            let _ = transcript_file.flush();
        }
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
        sleep(delay);
    }
    for entry in transcript_lines {
        let _ = writeln!(transcript_file, "{entry}");
        let _ = transcript_file.flush();
        sleep(delay);
    }

    let code: u8 = env::var(EXIT_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if code != 0 {
        eprintln!("fake claude failing on purpose with exit code {code}");
    }
    ExitCode::from(code)
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
    let config_dir = match env::var_os("CLAUDE_CONFIG_DIR") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home_dir()?.join(".claude"),
    };
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
