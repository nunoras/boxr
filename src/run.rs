use crate::clock::{iso8601, now_millis};
use crate::detached::{self, LaunchFile};
use crate::fail::Fail;
use crate::harness::{apply_config_dir, Harness, HarnessSession, LaunchRequest, StreamEvent};
use crate::home::restrict_file;
use crate::ledger::{self, Follower, Seed, SessionStart, Summary};
use crate::report::{self, Ledger, Report};
use crate::session::Session;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Once, PoisonError, Weak};
use std::time::{Duration, Instant};

const STOP_POLL: Duration = Duration::from_millis(50);

pub struct Launch {
    pub session: Session,
    program: PathBuf,
}

pub fn prepare(harness: &Arc<dyn Harness>, request: &LaunchRequest, home: &Path) -> Result<Launch> {
    let session = Session::plan(home);
    let program = program_of(harness, request, &session)?;
    session.materialize()?;
    Ok(Launch { session, program })
}

pub fn harness_session_of(session: &Session) -> HarnessSession {
    HarnessSession {
        session_id: session.id.clone(),
        dir: session.harness_dir(),
    }
}

pub fn adopt(
    harness: &Arc<dyn Harness>,
    request: &LaunchRequest,
    session: Session,
) -> Result<Launch> {
    let program = program_of(harness, request, &session)?;
    session.materialize()?;
    Ok(Launch { session, program })
}

fn program_of(
    harness: &Arc<dyn Harness>,
    request: &LaunchRequest,
    session: &Session,
) -> Result<PathBuf> {
    let command = harness.command(request, &harness_session_of(session))?;
    locate(&command.program).ok_or_else(|| {
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
        .into()
    })
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

pub struct Account<'a> {
    pub name: Option<&'a str>,
    pub dir: Option<&'a Path>,
}

pub fn headless(
    harness: &Arc<dyn Harness>,
    request: &LaunchRequest,
    account: Account<'_>,
    launch: Launch,
) -> Result<Report> {
    let Launch { session, program } = launch;
    let harness_session = harness_session_of(&session);
    let mut command = harness.command(request, &harness_session)?;
    apply_config_dir(&mut command, harness.as_ref(), account.dir);
    let started = Instant::now();
    let started_at = now_millis();
    detached::record_launch(
        &session,
        &LaunchFile {
            harness: harness.id().to_string(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            prompt: request.prompt.clone(),
            cwd: request.cwd.clone(),
            account: account.name.map(str::to_string),
            started_millis: started_at as u64,
        },
    )?;
    detached::record_supervisor(&session, std::process::id())?;

    let mut builder = Command::new(&program);
    builder
        .args(&command.args)
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .current_dir(&request.cwd)
        .stdin(if command.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    die_with_parent(&mut builder);
    let mut spawned = builder
        .spawn()
        .with_context(|| format!("starting harness {}", program.display()))?;
    let _job = guard_children(&spawned);

    let stdin_pipe = spawned.stdin.take();
    let stdout = spawned
        .stdout
        .take()
        .ok_or_else(|| anyhow!("harness stdout was not captured"))?;
    let mut stderr = spawned
        .stderr
        .take()
        .ok_or_else(|| anyhow!("harness stderr was not captured"))?;
    let child = Arc::new(Mutex::new(Supervised(spawned)));
    let stop = watch_for_stop(&child);
    watch_stop_file(&stop, session.stop_path());

    let follower = Follower::start(
        Arc::clone(harness),
        harness_session.clone(),
        session.normalized_path(),
        Seed {
            session_id: session.id.clone(),
            harness_id: harness.id().to_string(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            mode: "headless".to_string(),
            profile: account.name.map(str::to_string),
        },
        account.dir.map(Path::to_path_buf),
    );
    let stream_path = session.stream_path();
    let mut stream_file = create_private_file(&stream_path)?;
    let stderr_path = session.stderr_path();
    let mut stderr_file = create_private_file(&stderr_path)?;

    let stdin_thread = match (command.stdin, stdin_pipe) {
        (Some(input), Some(pipe)) => Some(std::thread::spawn(move || deliver(pipe, input))),
        _ => None,
    };

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

    let status = wait_for(&child)?;
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

    let ledger = match record_transcript(
        harness.as_ref(),
        &harness_session,
        harness_session_id.as_deref(),
        account.dir,
        &session,
    ) {
        Ok(path) => Ledger::Recorded(path),
        Err(error) => Ledger::Failed(format!("{error:#}")),
    };

    let exit_code = exit_code_of(&status);
    let duration_ms = started.elapsed().as_millis() as u64;
    let summary = Summary {
        id: session.id.clone(),
        harness: harness.id().to_string(),
        harness_session_id: harness_session_id.clone(),
        model: request.model.clone(),
        effort: request.effort.clone(),
        profile: account.name.map(str::to_string),
        mode: "headless".to_string(),
        start: iso8601(started_at),
        end: iso8601(now_millis()),
        duration_ms,
        status: status_of(&status, stop.was_requested()).to_string(),
        exit_code,
        steps: tally.steps,
        prompt_tokens: tally.prompt_tokens,
        completion_tokens: tally.completion_tokens,
        cached_tokens: tally.cached_tokens,
    };
    let mut summary_error = ledger::append_summary(&session.summary_path(), &summary)
        .err()
        .map(|error| format!("{error:#}"));

    let report = Report {
        id: session.id.clone(),
        status: summary.status.clone(),
        harness: harness.id().to_string(),
        model: request.model.clone(),
        effort: request.effort.clone(),
        account: account.name.map(str::to_string),
        harness_session_id,
        start: summary.start.clone(),
        end: summary.end.clone(),
        duration_ms,
        exit_code,
        final_message,
        steps: tally.steps,
        prompt_tokens: tally.prompt_tokens,
        completion_tokens: tally.completion_tokens,
        cached_tokens: tally.cached_tokens,
        capture_error: tally.error,
        summary_error: summary_error.clone(),
        ledger,
    };
    if let Err(error) = report::write(&session.report_path(), &report) {
        summary_error.get_or_insert_with(|| format!("{error:#}"));
    }
    stop.finalize();

    Ok(Report {
        summary_error,
        ..report
    })
}

fn status_of(status: &ExitStatus, stopped_by_boxr: bool) -> &'static str {
    if status.success() {
        return "ok";
    }
    if stopped_by_boxr || stopped_externally(status) {
        return "interrupted";
    }
    "failed"
}

#[cfg(unix)]
fn stopped_externally(status: &ExitStatus) -> bool {
    const HANGUP: i32 = 1;
    const INTERRUPT: i32 = 2;
    const KILL: i32 = 9;
    const TERMINATE: i32 = 15;
    matches!(
        signal_of(status),
        Some(HANGUP | INTERRUPT | KILL | TERMINATE)
    )
}

#[cfg(not(unix))]
fn stopped_externally(status: &ExitStatus) -> bool {
    const STATUS_CONTROL_C_EXIT: i32 = -1_073_741_510;
    status.code() == Some(STATUS_CONTROL_C_EXIT)
}

#[cfg(target_os = "linux")]
fn die_with_parent(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(1);
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn die_with_parent(_command: &mut Command) {}

#[cfg(windows)]
fn guard_children(child: &Child) -> Option<crate::job::JobGuard> {
    crate::job::guard(child).ok()
}

#[cfg(not(windows))]
fn guard_children(_child: &Child) -> Option<()> {
    None
}

struct StopRequest {
    child: Weak<Mutex<Supervised>>,
    requested: AtomicBool,
    finalized: Mutex<bool>,
    finalized_changed: Condvar,
}

impl StopRequest {
    fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.upgrade() {
            let _ = child.lock().map(|mut supervised| supervised.0.kill());
        }
    }

    fn was_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    fn finalize(&self) {
        *self
            .finalized
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        self.finalized_changed.notify_all();
    }

    fn is_finalized(&self) -> bool {
        *self
            .finalized
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(windows)]
    fn await_finalized(&self, budget: Duration) {
        let finalized = self
            .finalized
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _ = self
            .finalized_changed
            .wait_timeout_while(finalized, budget, |finalized| !*finalized);
    }
}

static ACTIVE_STOP: Mutex<Option<Arc<StopRequest>>> = Mutex::new(None);
static STOP_HANDLER: Once = Once::new();

fn watch_for_stop(child: &Arc<Mutex<Supervised>>) -> Arc<StopRequest> {
    let stop = Arc::new(StopRequest {
        child: Arc::downgrade(child),
        requested: AtomicBool::new(false),
        finalized: Mutex::new(false),
        finalized_changed: Condvar::new(),
    });
    *ACTIVE_STOP.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&stop));
    STOP_HANDLER.call_once(install_stop_handler);
    stop
}

fn watch_stop_file(stop: &Arc<StopRequest>, path: PathBuf) {
    let stop = Arc::clone(stop);
    std::thread::spawn(move || {
        while !stop.is_finalized() {
            if path.exists() {
                stop.request();
                return;
            }
            std::thread::sleep(STOP_POLL);
        }
    });
}

fn active_stop() -> Option<Arc<StopRequest>> {
    ACTIVE_STOP
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[cfg(unix)]
fn install_stop_handler() {
    let _ = ctrlc::set_handler(|| {
        if let Some(stop) = active_stop() {
            stop.request();
        }
    });
}

#[cfg(windows)]
fn install_stop_handler() {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    };
    const CLOSE_BUDGET: Duration = Duration::from_millis(4500);

    unsafe extern "system" fn on_console_event(event: u32) -> BOOL {
        if let Some(stop) = active_stop() {
            stop.request();
            if matches!(
                event,
                CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
            ) {
                stop.await_finalized(CLOSE_BUDGET);
            }
        }
        1
    }

    unsafe {
        SetConsoleCtrlHandler(Some(on_console_event), 1);
    }
}

fn wait_for(child: &Arc<Mutex<Supervised>>) -> Result<ExitStatus> {
    loop {
        {
            let mut supervised = child
                .lock()
                .map_err(|_| anyhow!("the harness supervisor lock was poisoned"))?;
            if let Some(status) = supervised
                .0
                .try_wait()
                .context("waiting for the harness to exit")?
            {
                return Ok(status);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
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
    harness_session: &HarnessSession,
    harness_session_id: Option<&str>,
    account: Option<&Path>,
    session: &Session,
) -> Result<PathBuf> {
    let id = harness_session_id.ok_or_else(|| anyhow!("the harness reported no session id"))?;
    let source = harness.transcript(harness_session, id, account)?;
    let target = session.transcript_path();
    fs::copy(&source, &target)
        .with_context(|| format!("copying {} to {}", source.display(), target.display()))?;
    restrict_file(&target)?;
    Ok(target)
}

pub fn locate(program: &str) -> Option<PathBuf> {
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

pub fn exit_code_of(status: &ExitStatus) -> i32 {
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
