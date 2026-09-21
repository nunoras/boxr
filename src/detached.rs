use crate::clock::{iso8601, now_millis};
use crate::cost;
use crate::fail::Fail;
use crate::home::{restrict_file, write_file_atomically};
use crate::ledger::{self, Summary};
use crate::report::{self, Ledger, Report};
use crate::session::Session;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const SUPERVISOR_COMMAND: &str = "__supervise";
pub const POLL: Duration = Duration::from_millis(50);

fn default_headless_mode() -> String {
    "headless".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchFile {
    pub harness: String,
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub cwd: std::path::PathBuf,
    pub started_millis: u64,
    #[serde(default = "default_headless_mode")]
    pub mode: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub resumed_from: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, rename = "kindSource")]
    pub kind_source: Option<String>,
    #[serde(default, rename = "gitBase")]
    pub git_base: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorFile {
    pub pid: u32,
}

pub struct Running {
    pub pid: Option<u32>,
    pub launch: LaunchFile,
    pub steps: u64,
}

pub enum State {
    Running(Box<Running>),
    Finished(Box<Report>),
}

pub fn record_launch(session: &Session, launch: &LaunchFile) -> Result<()> {
    write_record(&session.launch_path(), launch)
}

pub fn read_launch(session: &Session) -> Result<Option<LaunchFile>> {
    read_record(&session.launch_path())
}

pub fn read_report(session: &Session) -> Result<Option<Report>> {
    read_record(&session.report_path())
}

pub fn record_supervisor(session: &Session, pid: u32) -> Result<()> {
    if std::env::var_os("BOXR_TEST_FAIL_SUPERVISOR_RECORD").is_some() {
        return Err(anyhow!("refusing to record the supervisor"));
    }
    write_record(&session.supervisor_path(), &SupervisorFile { pid })
}

pub fn spawn_supervisor(session: &Session) -> Result<Child> {
    let program =
        std::env::current_exe().context("locating the boxr executable to supervise with")?;
    let log = session.dir.join("supervisor.log");
    let stderr = fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
    restrict_file(&log)?;
    let mut command = Command::new(program);
    command
        .arg(SUPERVISOR_COMMAND)
        .arg(&session.id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    detach(&mut command);
    command
        .spawn()
        .context("starting the boxr supervisor process")
}

pub fn request_stop(session: &Session) -> Result<()> {
    let path = session.stop_path();
    fs::write(&path, b"").with_context(|| format!("writing {}", path.display()))
}

pub fn abandon_detach(session: &Session, supervisor: Option<Child>) {
    if let Some(mut child) = supervisor {
        let _ = child.kill();
        let _ = child.wait();
    }
    let launch = read_launch(session).ok().flatten();
    let report = interrupted(session, launch.as_ref());
    let _ = report::write(&session.report_path(), &report);
    append_summary(session, &report);
}

pub fn state(home: &Path, id: &str) -> Result<State> {
    let session = Session::open(home, id);
    let launch = read_record::<LaunchFile>(&session.launch_path())?;
    let supervisor = read_record::<SupervisorFile>(&session.supervisor_path())?;
    if let Some(supervisor) = &supervisor {
        if alive(supervisor.pid) {
            let launch = launch.ok_or_else(|| anyhow!("session {id} has no launch record"))?;
            let steps = ledger::totals(&session.normalized_path()).steps;
            return Ok(State::Running(Box::new(Running {
                pid: Some(supervisor.pid),
                launch,
                steps,
            })));
        }
    }
    if let Some(summary) = ledger::find_summary(home, id)? {
        return Ok(State::Finished(Box::new(finished(&session, &summary)?)));
    }
    if let Some(report) = read_record::<Report>(&session.report_path())? {
        append_summary(&session, &report);
        return Ok(State::Finished(Box::new(report)));
    }
    if supervisor.is_some() {
        let report = interrupted(&session, launch.as_ref());
        append_summary(&session, &report);
        return Ok(State::Finished(Box::new(report)));
    }
    if let Some(launch) = launch {
        let steps = ledger::totals(&session.normalized_path()).steps;
        return Ok(State::Running(Box::new(Running {
            pid: None,
            launch,
            steps,
        })));
    }
    Err(no_session(id))
}

fn no_session(id: &str) -> anyhow::Error {
    Fail::usage(
        format!("no session {id} in the ledger"),
        vec!["Run a boxr launch, then `boxr ps` to list the sessions it recorded".to_string()],
    )
    .into()
}

pub fn running(session: &Session) -> Result<bool> {
    match read_record::<SupervisorFile>(&session.supervisor_path())? {
        Some(supervisor) => Ok(alive(supervisor.pid)),
        None => Ok(session.launch_path().is_file() && !session.report_path().is_file()),
    }
}

pub fn settled(home: &Path, id: &str) -> Result<Option<Report>> {
    let session = Session::open(home, id);
    if running(&session)? {
        return Ok(None);
    }
    match state(home, id)? {
        State::Finished(report) => Ok(Some(*report)),
        State::Running(_) => Ok(None),
    }
}

pub fn finish(home: &Path, id: &str, budget: Duration) -> Result<Report> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(report) = settled(home, id)? {
            return Ok(report);
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("session {id} is still running"));
        }
        std::thread::sleep(POLL);
    }
}

pub fn hard_kill(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(windows)]
    terminate_windows(pid);
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0;
        let read = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        read != 0 && code == STILL_ACTIVE as u32
    }
}

#[cfg(windows)]
fn terminate_windows(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    keep_caller_pipes_out_of_children();
}

#[cfg(windows)]
fn keep_caller_pipes_out_of_children() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            SetHandleInformation(GetStdHandle(stream), HANDLE_FLAG_INHERIT, 0);
        }
    }
}

fn finished(session: &Session, summary: &Summary) -> Result<Report> {
    if let Some(report) = read_record::<Report>(&session.report_path())? {
        return Ok(report);
    }
    let transcript = session.transcript_path();
    let ledger = if transcript.is_file() {
        Ledger::Recorded(transcript)
    } else {
        Ledger::Interrupted
    };
    Ok(Report {
        id: summary.id.clone(),
        status: summary.status.clone(),
        harness: summary.harness.clone(),
        model: summary.model.clone(),
        effort: summary.effort.clone(),
        harness_session_id: summary.harness_session_id.clone(),
        mode: summary.mode.clone(),
        profile: summary.profile.clone(),
        resumed_from: summary.resumed_from.clone(),
        kind: summary.kind.clone(),
        kind_source: summary.kind_source.clone(),
        start: summary.start.clone(),
        end: summary.end.clone(),
        duration_ms: summary.duration_ms,
        exit_code: summary.exit_code,
        final_message: None,
        steps: summary.steps,
        prompt_tokens: summary.prompt_tokens,
        completion_tokens: summary.completion_tokens,
        cached_tokens: summary.cached_tokens,
        reasoning_tokens: summary.reasoning_tokens,
        api_equivalent_cost: summary.api_equivalent_cost,
        currency: summary.currency.clone(),
        cost_error: summary.cost_error.clone(),
        capture_error: None,
        summary_error: None,
        error: summary.error.clone(),
        stderr_tail: None,
        limit_hit: summary.limit_hit,
        interrupted: summary.interrupted,
        ledger,
    })
}

fn interrupted(session: &Session, launch: Option<&LaunchFile>) -> Report {
    let totals = ledger::totals(&session.normalized_path());
    let end = now_millis();
    let started = launch
        .map(|launch| launch.started_millis as u128)
        .unwrap_or(end);
    let model = launch
        .map(|launch| launch.model.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let pricing = cost::calculate(
        &session.home,
        &model,
        &cost::Tokens {
            prompt: totals.prompt_tokens,
            completion: totals.completion_tokens,
            cached: totals.cached_tokens,
            reasoning: totals.reasoning_tokens,
        },
    );
    Report {
        id: session.id.clone(),
        status: "interrupted".to_string(),
        harness: launch
            .map(|launch| launch.harness.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        model,
        effort: launch.and_then(|launch| launch.effort.clone()),
        harness_session_id: None,
        mode: launch
            .map(|launch| launch.mode.clone())
            .unwrap_or_else(default_headless_mode),
        profile: launch.and_then(|launch| launch.profile.clone()),
        resumed_from: launch.and_then(|launch| launch.resumed_from.clone()),
        kind: launch.and_then(|launch| launch.kind.clone()),
        kind_source: launch.and_then(|launch| launch.kind_source.clone()),
        start: iso8601(started),
        end: iso8601(end),
        duration_ms: end.saturating_sub(started) as u64,
        exit_code: interrupted_exit_code(),
        final_message: None,
        steps: totals.steps,
        prompt_tokens: totals.prompt_tokens,
        completion_tokens: totals.completion_tokens,
        cached_tokens: totals.cached_tokens,
        reasoning_tokens: totals.reasoning_tokens,
        api_equivalent_cost: pricing.api_equivalent_cost,
        currency: pricing.currency.clone(),
        cost_error: pricing.error.clone(),
        capture_error: totals.error,
        summary_error: None,
        error: None,
        stderr_tail: None,
        limit_hit: false,
        interrupted: true,
        ledger: Ledger::Interrupted,
    }
}

#[cfg(unix)]
fn interrupted_exit_code() -> i32 {
    128 + 9
}

#[cfg(not(unix))]
fn interrupted_exit_code() -> i32 {
    1
}

fn append_summary(session: &Session, report: &Report) {
    let git = read_launch(session)
        .ok()
        .flatten()
        .and_then(|launch| crate::git::collect(&launch.cwd, launch.git_base.as_deref()));
    let summary = Summary {
        id: report.id.clone(),
        harness: report.harness.clone(),
        harness_session_id: report.harness_session_id.clone(),
        model: report.model.clone(),
        effort: report.effort.clone(),
        profile: report.profile.clone(),
        resumed_from: report.resumed_from.clone(),
        mode: report.mode.clone(),
        start: report.start.clone(),
        end: report.end.clone(),
        duration_ms: report.duration_ms,
        status: report.status.clone(),
        exit_code: report.exit_code,
        steps: report.steps,
        prompt_tokens: report.prompt_tokens,
        completion_tokens: report.completion_tokens,
        cached_tokens: report.cached_tokens,
        reasoning_tokens: report.reasoning_tokens,
        api_equivalent_cost: report.api_equivalent_cost,
        currency: report.currency.clone(),
        cost_error: report.cost_error.clone(),
        kind: report.kind.clone(),
        kind_source: report.kind_source.clone(),
        interrupted: report.interrupted,
        limit_hit: report.limit_hit,
        error: report.error.clone(),
        verdict: None,
        verdict_note: None,
        git,
    };
    let _ = ledger::append_summary(&session.summary_path(), &summary);
}

fn write_record(path: &Path, value: &impl Serialize) -> Result<()> {
    let text = serde_json::to_string(value).context("encoding a session record")?;
    write_file_atomically(path, &format!("{text}\n"))
}

fn read_record<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::Error::from(error).context(format!("reading {}", path.display())))
        }
    };
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("parsing {}", path.display()))
}
