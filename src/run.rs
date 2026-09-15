use crate::clock::{iso8601, now_millis};
use crate::fail::Fail;
use crate::harness::{Harness, LaunchRequest, StreamEvent};
use crate::home::restrict_file;
use crate::ledger::{self, Follower, Seed, SessionStart, Summary, Tally};
use crate::session::Session;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Instant;

pub enum Ledger {
    Recorded(PathBuf),
    Failed(String),
}

pub struct Outcome {
    pub session: Session,
    pub exit_code: i32,
    pub harness_session_id: Option<String>,
    pub final_message: Option<String>,
    pub ledger: Ledger,
    pub duration_ms: u128,
    pub tally: Tally,
    pub summary: Summary,
    pub summary_path: PathBuf,
    pub summary_error: Option<String>,
}

impl Outcome {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}

struct Supervised(Child);

impl Drop for Supervised {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub fn headless(
    harness: &Arc<dyn Harness>,
    request: &LaunchRequest,
    home: &Path,
) -> Result<Outcome> {
    let command = harness.command(request)?;
    let program = locate(&command.program).ok_or_else(|| {
        Fail::harness_unavailable(
            format!(
                "harness executable `{}` was not found on PATH",
                command.program
            ),
            vec![format!(
                "Install {} or put its executable on PATH",
                harness.id()
            )],
        )
    })?;

    let started = Instant::now();
    let started_at = now_millis();
    let mut child = Supervised(
        Command::new(&program)
            .args(&command.args)
            .current_dir(&request.cwd)
            .stdin(if command.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting harness {}", program.display()))?,
    );

    let session = Session::create(home)?;
    let follower = Follower::start(
        Arc::clone(harness),
        session.normalized_path(),
        Seed {
            session_id: session.id.clone(),
            harness_id: harness.id().to_string(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            mode: "headless".to_string(),
            profile: None,
        },
    );
    let stream_path = session.stream_path();
    let mut stream_file = create_private_file(&stream_path)?;
    let stderr_path = session.stderr_path();
    let mut stderr_file = create_private_file(&stderr_path)?;

    let stdin_thread = match (command.stdin, child.0.stdin.take()) {
        (Some(input), Some(pipe)) => Some(std::thread::spawn(move || deliver(pipe, input))),
        _ => None,
    };

    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| anyhow!("harness stdout was not captured"))?;
    let mut stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| anyhow!("harness stderr was not captured"))?;
    let stderr_thread = std::thread::spawn(move || std::io::copy(&mut stderr, &mut stderr_file));

    let mut harness_session_id = None;
    let mut final_message = None;
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .context("reading harness stdout")?
            == 0
        {
            break;
        }
        stream_file
            .write_all(&line)
            .and_then(|()| stream_file.flush())
            .with_context(|| format!("writing {}", stream_path.display()))?;
        match harness.parse_event(String::from_utf8_lossy(&line).trim_end()) {
            StreamEvent::SessionStarted {
                harness_session_id: id,
                harness_version,
                model,
            } => {
                harness_session_id = Some(id.clone());
                follower.session_started(SessionStart {
                    harness_session_id: id,
                    harness_version,
                    model,
                });
            }
            StreamEvent::FinalMessage { text } => final_message = Some(text),
            StreamEvent::Ignored => {}
        }
    }

    let status = child.0.wait().context("waiting for the harness to exit")?;
    if let Some(thread) = stdin_thread {
        thread
            .join()
            .map_err(|_| anyhow!("the stdin writer thread panicked"))?
            .context("writing the harness input")?;
    }
    stderr_thread
        .join()
        .map_err(|_| anyhow!("the stderr reader thread panicked"))?
        .with_context(|| format!("writing {}", stderr_path.display()))?;

    let tally = follower.finish();

    let ledger = match record_transcript(harness.as_ref(), harness_session_id.as_deref(), &session)
    {
        Ok(path) => Ledger::Recorded(path),
        Err(error) => Ledger::Failed(format!("{error:#}")),
    };

    let exit_code = exit_code_of(&status);
    let duration_ms = started.elapsed().as_millis();
    let ended_at = now_millis();
    let summary = Summary {
        id: session.id.clone(),
        harness: harness.id().to_string(),
        harness_session_id: harness_session_id.clone(),
        model: request.model.clone(),
        effort: request.effort.clone(),
        profile: None,
        mode: "headless".to_string(),
        start: iso8601(started_at),
        end: iso8601(ended_at),
        duration_ms: duration_ms as u64,
        status: status_of(&status).to_string(),
        exit_code,
        steps: tally.steps,
        prompt_tokens: tally.prompt_tokens,
        completion_tokens: tally.completion_tokens,
        cached_tokens: tally.cached_tokens,
    };
    let summary_path = home.join(ledger::SUMMARY_FILE);
    let summary_error = ledger::append_summary(&summary_path, &summary)
        .err()
        .map(|error| format!("{error:#}"));

    Ok(Outcome {
        session,
        exit_code,
        harness_session_id,
        final_message,
        ledger,
        duration_ms,
        tally,
        summary,
        summary_path,
        summary_error,
    })
}

fn status_of(status: &ExitStatus) -> &'static str {
    match (status.success(), signal_of(status)) {
        (true, _) => "ok",
        (false, Some(_)) => "interrupted",
        (false, None) => "failed",
    }
}

fn deliver(mut pipe: ChildStdin, input: String) -> std::io::Result<()> {
    match pipe.write_all(input.as_bytes()) {
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

fn create_private_file(path: &Path) -> Result<File> {
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    restrict_file(path)?;
    Ok(file)
}

fn record_transcript(
    harness: &dyn Harness,
    harness_session_id: Option<&str>,
    session: &Session,
) -> Result<PathBuf> {
    let id = harness_session_id.ok_or_else(|| anyhow!("the harness reported no session id"))?;
    let source = harness.transcript(id)?;
    let target = session.transcript_path();
    fs::copy(&source, &target)
        .with_context(|| format!("copying {} to {}", source.display(), target.display()))?;
    restrict_file(&target)?;
    Ok(target)
}

fn locate(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|dir| {
            executable_names(program)
                .into_iter()
                .map(move |name| dir.join(name))
        })
        .find(|candidate| is_executable(candidate))
}

#[cfg(windows)]
fn executable_names(program: &str) -> Vec<String> {
    env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                ".com" | ".exe" | ".bat" | ".cmd"
            )
        })
        .map(|extension| format!("{program}{extension}"))
        .collect()
}

#[cfg(not(windows))]
fn executable_names(program: &str) -> Vec<String> {
    vec![program.to_string()]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn exit_code_of(status: &ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| signal_of(status).map(|signal| 128 + signal))
        .unwrap_or(1)
}

#[cfg(unix)]
fn signal_of(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_status: &ExitStatus) -> Option<i32> {
    None
}
