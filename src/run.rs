use crate::clock::{iso8601, now_millis};
use crate::cost;
use crate::detached::{self, LaunchFile};
use crate::fail::Fail;
use crate::git;
use crate::harness::{apply_config_dir, Harness, HarnessSession, LaunchRequest, StreamEvent};
use crate::home::restrict_file;
use crate::ledger::{self, Follower, Seed, SessionStart, Summary};
use crate::report::{self, Ledger, Report};
use crate::session::Session;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Once, PoisonError, Weak};
use std::time::{Duration, Instant};

const STOP_POLL: Duration = Duration::from_millis(50);
const STDERR_TAIL_BYTES: u64 = 4096;

pub struct Launch {
    pub session: Session,
    program: PathBuf,
    harness_session: HarnessSession,
    mode: String,
    profile: Option<String>,
    resumed_from: Option<String>,
    from_bytes: u64,
}

pub struct Continuation {
    pub parent: String,
    pub profile: Option<String>,
    pub from_bytes: u64,
}

pub fn prepare(harness: &Arc<dyn Harness>, request: &LaunchRequest, home: &Path) -> Result<Launch> {
    let session = Session::plan(home);
    let harness_session = harness_session_of(&session);
    let program = program_of(harness, request, &harness_session)?;
    session.materialize()?;
    Ok(Launch {
        session,
        program,
        harness_session,
        mode: "headless".to_string(),
        profile: None,
        resumed_from: None,
        from_bytes: 0,
    })
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
    let harness_session = harness_session_of(&session);
    let program = program_of(harness, request, &harness_session)?;
    session.materialize()?;
    Ok(Launch {
        session,
        program,
        harness_session,
        mode: "headless".to_string(),
        profile: None,
        resumed_from: None,
        from_bytes: 0,
    })
}

pub fn resume(
    harness: &Arc<dyn Harness>,
    request: &LaunchRequest,
    continuation: &Continuation,
    home: &Path,
) -> Result<Launch> {
    let session = Session::plan(home);
    let harness_session = continued_harness_session(request, &session, home, &continuation.parent)?;
    let program = program_of(harness, request, &harness_session)?;
    session.materialize()?;
    Ok(Launch {
        session,
        program,
        harness_session,
        mode: "resume".to_string(),
        profile: continuation.profile.clone(),
        resumed_from: Some(continuation.parent.clone()),
        from_bytes: continuation.from_bytes,
    })
}

pub fn origin_session(home: &Path, id: &str) -> Result<Session> {
    let mut current = id.to_string();
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return Err(anyhow!("resume chain for {id} loops back to {current}"));
        }
        let summary = ledger::read_summary(home, &current)?;
        match summary.resumed_from {
            Some(parent) => current = parent,
            None => return Ok(Session::open(home, &current)),
        }
    }
}

fn continued_harness_session(
    request: &LaunchRequest,
    session: &Session,
    home: &Path,
    parent_id: &str,
) -> Result<HarnessSession> {
    match &request.mode {
        crate::harness::LaunchMode::Resume { harness_session_id } => {
            let origin = origin_session(home, parent_id)?;
            Ok(HarnessSession {
                session_id: harness_session_id.clone(),
                dir: origin.harness_dir(),
            })
        }
        crate::harness::LaunchMode::Fresh => Ok(harness_session_of(session)),
    }
}

pub fn transcript_size(
    harness: &dyn Harness,
    session: &HarnessSession,
    harness_session_id: &str,
    account: Option<&Path>,
) -> Result<u64> {
    let path = harness.transcript(session, harness_session_id, account)?;
    let size = fs::metadata(&path)
        .with_context(|| format!("reading {}", path.display()))?
        .len();
    Ok(size)
}

fn program_of(
    harness: &Arc<dyn Harness>,
    request: &LaunchRequest,
    harness_session: &HarnessSession,
) -> Result<PathBuf> {
    let command = harness.command(request, harness_session)?;
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
    let Launch {
        session,
        program,
        harness_session,
        mode,
        profile,
        resumed_from,
        from_bytes,
    } = launch;
    let profile = profile.or_else(|| account.name.map(str::to_string));
    let mut command = harness.command(request, &harness_session)?;
    apply_config_dir(&mut command, harness.as_ref(), account.dir);
    let started = Instant::now();
    let started_at = now_millis();
    let git_base = detached::read_launch(&session)?
        .and_then(|launch| launch.git_base)
        .or_else(|| git::head(&request.cwd));
    detached::record_launch(
        &session,
        &LaunchFile {
            harness: harness.id().to_string(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            prompt: request.prompt.clone(),
            cwd: request.cwd.clone(),
            started_millis: started_at as u64,
            mode: mode.clone(),
            profile: profile.clone(),
            resumed_from: resumed_from.clone(),
            kind: request.kind.clone(),
            kind_source: request.kind_source.clone(),
            git_base: git_base.clone(),
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
    #[cfg(windows)]
    let _job = match guard_children(&spawned) {
        Ok(job) => job,
        Err(error) => {
            let _ = spawned.kill();
            let _ = spawned.wait();
            return Err(error);
        }
    };
    #[cfg(not(windows))]
    if let Err(error) = guard_children(&spawned) {
        let _ = spawned.kill();
        let _ = spawned.wait();
        return Err(error);
    }

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
            mode: mode.clone(),
            profile: profile.clone(),
            resumed_from: resumed_from.clone(),
            kind: request.kind.clone(),
            kind_source: request.kind_source.clone(),
        },
        account.dir.map(Path::to_path_buf),
        from_bytes,
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
    let mut harness_error = None;
    let mut hit_limit = false;
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
            StreamEvent::FinalMessage {
                text,
                error,
                limit_hit,
            } => {
                if let Some(text) = text {
                    final_message = Some(text);
                }
                if let Some(error) = error {
                    harness_error = Some(error);
                }
                if limit_hit {
                    hit_limit = true;
                }
            }
            StreamEvent::Recovered => {
                harness_error = None;
                hit_limit = false;
            }
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
    let interrupted = stop.was_requested() || stopped_externally(&status);
    let stderr_tail = match &harness_error {
        Some(_) => None,
        None if !interrupted && !status.success() => read_stderr_tail(&stderr_path),
        None => None,
    };
    let git_evidence = git::collect(&request.cwd, git_base.as_deref());
    let pricing = cost::calculate(
        &session.home,
        &request.model,
        &cost::Tokens {
            prompt: tally.prompt_tokens,
            completion: tally.completion_tokens,
            cached: tally.cached_tokens,
            reasoning: tally.reasoning_tokens,
        },
    );
    let summary = Summary {
        id: session.id.clone(),
        harness: harness.id().to_string(),
        harness_session_id: harness_session_id.clone(),
        model: request.model.clone(),
        effort: request.effort.clone(),
        profile,
        resumed_from: resumed_from.clone(),
        mode,
        start: iso8601(started_at),
        end: iso8601(now_millis()),
        duration_ms,
        status: status_of(&status, interrupted, harness_error.is_some() || hit_limit).to_string(),
        exit_code,
        steps: tally.steps,
        prompt_tokens: tally.prompt_tokens,
        completion_tokens: tally.completion_tokens,
        cached_tokens: tally.cached_tokens,
        interrupted,
        limit_hit: hit_limit,
        error: harness_error.clone(),
        verdict: None,
        verdict_note: None,
        git: git_evidence,
        reasoning_tokens: tally.reasoning_tokens,
        api_equivalent_cost: pricing.api_equivalent_cost,
        currency: pricing.currency.clone(),
        cost_error: pricing.error.clone(),
        kind: request.kind.clone(),
        kind_source: request.kind_source.clone(),
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
        harness_session_id,
        mode: summary.mode.clone(),
        profile: summary.profile.clone(),
        resumed_from,
        start: summary.start.clone(),
        end: summary.end.clone(),
        duration_ms,
        exit_code,
        final_message,
        steps: tally.steps,
        prompt_tokens: tally.prompt_tokens,
        completion_tokens: tally.completion_tokens,
        cached_tokens: tally.cached_tokens,
        reasoning_tokens: tally.reasoning_tokens,
        api_equivalent_cost: pricing.api_equivalent_cost,
        currency: pricing.currency,
        cost_error: pricing.error,
        capture_error: tally.error,
        summary_error: summary_error.clone(),
        error: harness_error,
        stderr_tail,
        limit_hit: hit_limit,
        interrupted,
        ledger,
        kind: summary.kind.clone(),
        kind_source: summary.kind_source.clone(),
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

fn read_stderr_tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(STDERR_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    if start > 0 {
        let newline = bytes.iter().position(|byte| *byte == b'\n')?;
        bytes.drain(..=newline);
    }
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn status_of(status: &ExitStatus, interrupted: bool, harness_failed: bool) -> &'static str {
    if interrupted {
        return "interrupted";
    }
    if status.success() && !harness_failed {
        return "ok";
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
fn guard_children(child: &Child) -> Result<crate::job::JobGuard> {
    if env::var_os("BOXR_TEST_FAIL_JOB_GUARD").is_some() {
        std::thread::sleep(Duration::from_millis(200));
        return Err(anyhow!("refusing to launch without a job object guard"));
    }
    crate::job::guard(child)
}

#[cfg(not(windows))]
fn guard_children(_child: &Child) -> Result<()> {
    if env::var_os("BOXR_TEST_FAIL_JOB_GUARD").is_some() {
        std::thread::sleep(Duration::from_millis(200));
        return Err(anyhow!("refusing to launch without a process guard"));
    }
    Ok(())
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
