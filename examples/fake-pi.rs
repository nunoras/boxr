use serde_json::Value;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

const FIXTURE_ENV: &str = "BOXR_FAKE_PI_FIXTURE";
const EXIT_ENV: &str = "BOXR_FAKE_PI_EXIT";
const ARGS_ENV: &str = "BOXR_FAKE_PI_ARGS";
const PROMPT_ENV: &str = "BOXR_FAKE_PI_PROMPT";
const DELAY_ENV: &str = "BOXR_FAKE_PI_DELAY_MS";
const STDERR_ENV: &str = "BOXR_FAKE_PI_STDERR";
const MODELS_ENV: &str = "BOXR_FAKE_PI_MODELS";
const MODELS_EXIT_ENV: &str = "BOXR_FAKE_PI_MODELS_EXIT";
const MODELS_STDERR_ENV: &str = "BOXR_FAKE_PI_MODELS_STDERR";

const DEFAULT_CATALOG: &str = "\
provider      model                    context  max-out  thinking  images
xai           grok-4.5                 500K     500K     yes       yes
xai           grok-4.6                 500K     500K     yes       yes
anthropic     claude-sonnet-4-5        1M       64K      yes       yes
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Some(path) = env::var_os(ARGS_ENV) {
        if let Err(error) = fs::write(PathBuf::from(path), args.join("\n")) {
            eprintln!("cannot record arguments: {error}");
            return ExitCode::from(97);
        }
    }
    if args.iter().any(|arg| arg == "--list-models") {
        return list_models();
    }

    let fixture = match env::var_os(FIXTURE_ENV) {
        Some(value) => PathBuf::from(value),
        None => {
            eprintln!("{FIXTURE_ENV} is not set");
            return ExitCode::from(97);
        }
    };

    let mut prompt = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut prompt) {
        eprintln!("cannot read the prompt from stdin: {error}");
        return ExitCode::from(97);
    }
    if prompt.is_empty() {
        eprintln!("Error: no message provided");
        return ExitCode::from(97);
    }
    if let Some(path) = env::var_os(PROMPT_ENV) {
        if let Err(error) = fs::write(PathBuf::from(path), &prompt) {
            eprintln!("cannot record the prompt: {error}");
            return ExitCode::from(97);
        }
    }

    let session_id = match option(&args, "--session-id") {
        Some(value) => value,
        None => {
            eprintln!("boxr did not pass --session-id");
            return ExitCode::from(97);
        }
    };
    let session_dir = match option(&args, "--session-dir") {
        Some(value) => PathBuf::from(value),
        None => {
            eprintln!("boxr did not pass --session-dir");
            return ExitCode::from(97);
        }
    };

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
    let fixture_id = match header_value(&stream, "id") {
        Some(id) => id,
        None => {
            eprintln!("the fixture stream has no session id");
            return ExitCode::from(97);
        }
    };
    let stream = stream.replace(&fixture_id, &session_id);
    let transcript = transcript.replace(&fixture_id, &session_id);
    let file_timestamp = match header_value(&stream, "timestamp") {
        Some(timestamp) => timestamp.replace([':', '.'], "-"),
        None => {
            eprintln!("the fixture stream has no session timestamp");
            return ExitCode::from(97);
        }
    };

    if let Err(error) = fs::create_dir_all(&session_dir) {
        eprintln!("cannot create the session directory: {error}");
        return ExitCode::from(97);
    }
    let suffix = format!("_{session_id}.jsonl");
    let existing = existing_session_file(&session_dir, &suffix);
    let continuing = existing.is_some();
    let transcript_path = existing
        .unwrap_or_else(|| session_dir.join(format!("{file_timestamp}_{session_id}.jsonl")));

    let delay = Duration::from_millis(
        env::var(DELAY_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5),
    );

    let mut transcript_file = match open_transcript(&transcript_path, continuing) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("cannot open the transcript: {error}");
            return ExitCode::from(97);
        }
    };

    let mut stdout = std::io::stdout();
    let transcript_lines = transcript_entries(&transcript, continuing);
    let mut transcript_lines = transcript_lines.into_iter();
    for line in stream.lines() {
        if let Some(entry) = transcript_lines.next() {
            append(&mut transcript_file, &entry);
        }
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
        sleep(delay);
    }
    for entry in transcript_lines {
        append(&mut transcript_file, &entry);
        sleep(delay);
    }

    let code: u8 = env::var(EXIT_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    match env::var(STDERR_ENV) {
        Ok(text) if !text.is_empty() => eprintln!("{text}"),
        Ok(_) => {}
        Err(_) if code != 0 => eprintln!("fake pi failing on purpose with exit code {code}"),
        Err(_) => {}
    }
    ExitCode::from(code)
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn list_models() -> ExitCode {
    if let Ok(text) = env::var(MODELS_STDERR_ENV) {
        if !text.is_empty() {
            eprintln!("{text}");
        }
    }
    if let Ok(value) = env::var(MODELS_EXIT_ENV) {
        if let Ok(code) = value.parse::<u8>() {
            if code != 0 {
                return ExitCode::from(code);
            }
        }
    }
    let catalog = match env::var_os(MODELS_ENV) {
        Some(path) if !path.is_empty() => match fs::read_to_string(PathBuf::from(path)) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("cannot read the fake model catalog: {error}");
                return ExitCode::from(97);
            }
        },
        _ => DEFAULT_CATALOG.to_string(),
    };
    print!("{catalog}");
    ExitCode::from(0)
}

fn header_value(stream: &str, key: &str) -> Option<String> {
    let line = stream.lines().next()?;
    let value: Value = serde_json::from_str(line).ok()?;
    value.get(key)?.as_str().map(str::to_string)
}

fn append(file: &mut fs::File, entry: &str) {
    let _ = writeln!(file, "{entry}");
    let _ = file.flush();
}

fn existing_session_file(session_dir: &PathBuf, suffix: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(session_dir).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(suffix))
        })
        .collect();
    matches.sort();
    matches.pop()
}

fn open_transcript(path: &PathBuf, continuing: bool) -> std::io::Result<fs::File> {
    if continuing {
        fs::OpenOptions::new().append(true).open(path)
    } else {
        fs::File::create(path)
    }
}

fn transcript_entries(transcript: &str, continuing: bool) -> Vec<String> {
    transcript
        .lines()
        .filter(|line| {
            if !continuing {
                return true;
            }
            match serde_json::from_str::<Value>(line) {
                Ok(value) => value.get("type").and_then(Value::as_str) == Some("message"),
                Err(_) => true,
            }
        })
        .map(str::to_string)
        .collect()
}
