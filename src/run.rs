use crate::harness::{Harness, LaunchRequest, StreamEvent};
use crate::home::restrict_file;
use crate::session::Session;
use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

pub struct Outcome {
    pub exit_code: i32,
    pub harness_session_id: Option<String>,
    pub final_message: Option<String>,
    pub transcript: Option<PathBuf>,
    pub duration_ms: u128,
}

impl Outcome {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}

pub fn headless(
    harness: &dyn Harness,
    request: &LaunchRequest,
    session: &Session,
) -> Result<Outcome> {
    let command = harness.command(request)?;
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .envs(command.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(&request.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow!(
                    "harness executable `{}` was not found on PATH",
                    command.program
                )
            } else {
                anyhow::Error::from(error)
                    .context(format!("starting harness `{}`", command.program))
            }
        })?;

    let started = Instant::now();
    let stream_path = session.stream_path();
    let mut stream_file = File::create(&stream_path)
        .with_context(|| format!("creating {}", stream_path.display()))?;
    restrict_file(&stream_path)?;

    let mut harness_session_id = None;
    let mut final_message = None;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("harness stdout was not captured"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("harness stderr was not captured"))?;
    let stderr_path = session.stderr_path();
    let stderr_thread = std::thread::spawn(move || -> Result<()> {
        let mut file = File::create(&stderr_path)
            .with_context(|| format!("creating {}", stderr_path.display()))?;
        std::io::copy(&mut stderr, &mut file)?;
        restrict_file(&stderr_path)
    });

    for line in BufReader::new(stdout).lines() {
        let line = line.context("reading harness stdout")?;
        writeln!(stream_file, "{line}")
            .with_context(|| format!("writing {}", stream_path.display()))?;
        stream_file.flush()?;
        if !command.events_on_stdout {
            continue;
        }
        match harness.parse_event(&line) {
            StreamEvent::SessionStarted {
                harness_session_id: id,
            } => harness_session_id = Some(id),
            StreamEvent::FinalMessage { text } => final_message = Some(text),
            StreamEvent::Ignored => {}
        }
    }

    let status = child.wait().context("waiting for the harness to exit")?;
    stderr_thread
        .join()
        .map_err(|_| anyhow!("the stderr reader thread panicked"))??;

    let transcript = harness_session_id
        .as_deref()
        .and_then(|id| harness.transcript(&command, id))
        .and_then(|source| copy_transcript(&source, session).ok());

    Ok(Outcome {
        exit_code: exit_code_of(&status),
        harness_session_id,
        final_message,
        transcript,
        duration_ms: started.elapsed().as_millis(),
    })
}

fn copy_transcript(source: &std::path::Path, session: &Session) -> Result<PathBuf> {
    let target = session.transcript_path();
    fs::copy(source, &target)
        .with_context(|| format!("copying {} to {}", source.display(), target.display()))?;
    restrict_file(&target)?;
    Ok(target)
}

fn exit_code_of(status: &std::process::ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| signal_exit_code(status))
}

#[cfg(unix)]
fn signal_exit_code(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| 128 + signal).unwrap_or(1)
}

#[cfg(not(unix))]
fn signal_exit_code(_status: &std::process::ExitStatus) -> i32 {
    1
}
