mod account;
mod atif;
mod clock;
mod config;
mod detached;
mod fail;
mod git;
mod harness;
mod home;
#[cfg(windows)]
mod job;
mod ledger;
mod output;
mod report;
mod run;
mod session;
mod skill;
mod stats;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use config::Config;
use detached::{LaunchFile, State};
use fail::{Fail, EXIT_INTERNAL, EXIT_OK};
use harness::{LaunchMode, LaunchRequest};
use ledger::Summary;
use output::{one_line, Kind, Toon, MESSAGE_LIMIT};
use session::Session;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STOP_BUDGET: Duration = Duration::from_secs(10);
const KILL_BUDGET: Duration = Duration::from_secs(5);

#[derive(Parser, Debug)]
#[command(
    name = "boxr",
    version,
    about = "Launch coding agents and record every session in a local ledger",
    disable_help_subcommand = true,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, value_name = "HARNESS")]
    harness: Option<String>,

    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    #[arg(long, value_name = "EFFORT")]
    effort: Option<String>,

    #[arg(long, value_name = "NAME")]
    account: Option<String>,

    #[arg(long, value_name = "KIND")]
    kind: Option<String>,

    #[arg(long)]
    detach: bool,

    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,
}

#[derive(Parser, Debug)]
#[command(name = "boxr", disable_help_subcommand = true)]
struct SkillCli {
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Subcommand, Debug)]
enum SkillCommand {
    Skill(SkillNamespace),
}

#[derive(Args, Debug)]
struct SkillNamespace {
    #[command(subcommand)]
    command: SkillAction,
}

#[derive(Subcommand, Debug)]
enum SkillAction {
    Install {
        #[arg(long, value_name = "HARNESS")]
        harness: String,
    },
}

#[derive(Subcommand, Debug)]
enum Command {
    Show {
        #[arg(value_name = "ID")]
        id: String,
    },
    Export {
        #[arg(long)]
        atif: bool,

        #[arg(value_name = "ID")]
        id: String,
    },
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    Ps,
    Status {
        #[arg(value_name = "ID")]
        id: String,
    },
    Wait {
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<u64>,

        #[arg(value_name = "ID")]
        id: String,
    },
    Tail {
        #[arg(value_name = "ID")]
        id: String,
    },
    Stop {
        #[arg(value_name = "ID")]
        id: String,
    },
    Resume {
        #[arg(value_name = "ID")]
        id: String,

        #[arg(value_name = "PROMPT")]
        prompt: String,
    },
    /// Record the caller's verdict on a session, or check whether its commits were reverted
    ///
    /// Record a verdict with `boxr outcome <id> success|partial|failed [--note <text>]`.
    /// Check whether the commits a session made are still reachable on their branch with
    /// `boxr outcome --check-reverted <id>`; commits that no longer are get recorded as reverted.
    Outcome {
        #[arg(long)]
        check_reverted: bool,

        #[arg(value_name = "ID")]
        id: Option<String>,

        #[arg(value_name = "VERDICT")]
        verdict: Option<String>,

        #[arg(long, value_name = "NOTE")]
        note: Option<String>,
    },
    Stats {
        #[arg(long, value_name = "DIMS")]
        by: String,

        #[arg(long, value_name = "WINDOW")]
        since: String,
    },
    #[command(name = "__supervise", hide = true)]
    Supervise {
        #[arg(value_name = "ID")]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum AccountAction {
    Add(AccountAdd),
    List,
    Remove(AccountRemove),
}

#[derive(Args, Debug)]
struct AccountAdd {
    #[arg(long, value_name = "HARNESS")]
    harness: Option<String>,

    #[arg(long, value_name = "NAME")]
    name: String,
}

#[derive(Args, Debug)]
struct AccountRemove {
    #[arg(long, value_name = "HARNESS")]
    harness: Option<String>,

    #[arg(long, value_name = "NAME")]
    name: String,

    #[arg(long)]
    yes: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => ExitCode::from(report_error(&error) as u8),
    }
}

fn run() -> Result<i32> {
    if is_skill_install_command() {
        let cli = SkillCli::parse();
        return match cli.command {
            SkillCommand::Skill(SkillNamespace {
                command: SkillAction::Install { harness },
            }) => skill_install(&harness),
        };
    }
    dispatch()
}

fn is_skill_install_command() -> bool {
    let mut args = std::env::args_os().skip(1);
    args.next().as_deref() == Some(OsStr::new("skill"))
        && args.next().as_deref() == Some(OsStr::new("install"))
}

fn skill_install(harness: &str) -> Result<i32> {
    let installed = skill::install(harness)?;
    print!("{}", render_skill_install(&installed));
    Ok(EXIT_OK)
}

fn render_skill_install(installed: &[skill::Installed]) -> String {
    let skills: Vec<String> = skill::names().map(str::to_string).collect();
    let mut harnesses: Vec<&str> = installed.iter().map(|item| item.harness).collect();
    harnesses.dedup();
    let mut toon = Toon::new();
    toon.section("skill")
        .field("action", "install")
        .number("harnesses", harnesses.len());
    toon.list("skills", &skills);
    let rows: Vec<String> = installed
        .iter()
        .map(|item| format!("{}: {}", item.harness, item.dir.display()))
        .collect();
    toon.list("installed", &rows);
    let help = installed
        .first()
        .map(|item| {
            vec![
                format!("Read the skill at {}", item.dir.join("SKILL.md").display()),
                "Run `boxr skill install --harness all` to update every harness".to_string(),
            ]
        })
        .unwrap_or_default();
    toon.list("help", &help);
    toon.render()
}

fn dispatch() -> Result<i32> {
    let cli = Cli::parse();
    let home = home::ensure_home()?;
    let config = Config::load(&home)?;
    match cli.command {
        Some(Command::Show { id }) => show(&id),
        Some(Command::Export { atif, id }) => export(atif, &id),
        Some(Command::Account { action }) => accounts(action, &home, &config),
        Some(Command::Ps) => ps(),
        Some(Command::Status { id }) => status(&id),
        Some(Command::Wait { timeout, id }) => wait(&id, timeout),
        Some(Command::Tail { id }) => tail(&id),
        Some(Command::Stop { id }) => stop(&id),
        Some(Command::Resume { id, prompt }) => resume(&id, &prompt),
        Some(Command::Outcome {
            check_reverted,
            id,
            verdict,
            note,
        }) => outcome(check_reverted, id, verdict, note),
        Some(Command::Stats { by, since }) => stats(&home, &by, &since),
        Some(Command::Supervise { id }) => supervise(&id),
        None => launch(cli, &home, &config),
    }
}

fn launch(cli: Cli, home: &Path, config: &Config) -> Result<i32> {
    let prompt = cli.prompt.clone().ok_or_else(|| {
        Fail::usage(
            "no prompt given",
            vec!["Run `boxr --harness claude --model <m> \"<prompt>\"`".to_string()],
        )
    })?;

    let harness_id = resolve(
        "harness",
        cli.harness,
        config.defaults.harness.clone(),
        home,
    )?;
    let model = resolve("model", cli.model, config.defaults.model.clone(), home)?;
    let effort = cli.effort.or(config.defaults.effort.clone());
    let account = cli.account.or(config.defaults.account.clone());
    let kind = declared_kind(cli.kind.as_deref(), config)?;

    let adapter = adapter_for(&harness_id)?;

    let profile = match &account {
        Some(name) => Some(profile_for(adapter.as_ref(), home, &harness_id, name)?),
        None => None,
    };

    let cwd = std::env::current_dir().context("reading the current directory")?;
    let request = LaunchRequest {
        model,
        effort,
        prompt,
        cwd,
        mode: LaunchMode::Fresh,
        kind,
        kind_source: cli.kind.map(|_| "declared".to_string()),
    };

    if cli.detach {
        return detach(&adapter, &request, account.as_deref(), home);
    }

    let launch = run::prepare(&adapter, &request, home)?;
    let report = run::headless(
        &adapter,
        &request,
        run::Account {
            name: account.as_deref(),
            dir: profile.as_deref(),
        },
        launch,
    )?;
    print!(
        "{}",
        report::render(&report, &Session::open(home, &report.id))
    );
    Ok(report::exit_code(&report))
}

fn detach(
    harness: &Arc<dyn harness::Harness>,
    request: &LaunchRequest,
    account: Option<&str>,
    home: &Path,
) -> Result<i32> {
    let launch = run::prepare(harness, request, home)?;
    let session = launch.session;
    detached::record_launch(
        &session,
        &LaunchFile {
            harness: harness.id().to_string(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            prompt: request.prompt.clone(),
            cwd: request.cwd.clone(),
            started_millis: clock::now_millis() as u64,
            mode: "headless".to_string(),
            profile: account.map(str::to_string),
            resumed_from: None,
            kind: request.kind.clone(),
            kind_source: request.kind_source.clone(),
        },
    )?;
    let mut supervisor = match detached::spawn_supervisor(&session) {
        Ok(child) => child,
        Err(error) => {
            detached::abandon_detach(&session, None);
            return Err(error);
        }
    };
    let pid = supervisor.id();
    if let Err(error) = detached::record_supervisor(&session, pid) {
        detached::abandon_detach(&session, Some(supervisor));
        return Err(error);
    }
    let _ = supervisor.try_wait();
    print!("{}", render_detached(&session, harness, request, pid));
    Ok(EXIT_OK)
}

fn render_detached(
    session: &Session,
    harness: &Arc<dyn harness::Harness>,
    request: &LaunchRequest,
    pid: u32,
) -> String {
    let mut toon = Toon::new();
    toon.section("session")
        .field("id", &session.id)
        .field("status", "running")
        .field("harness", harness.id())
        .field("model", &request.model)
        .field(
            "effort",
            request.effort.as_deref().unwrap_or("harness-default"),
        )
        .number("pid", pid);
    toon.list(
        "help",
        &[
            format!("Run `boxr wait {}` to block until it finishes", session.id),
            format!("Run `boxr status {}` to check on it", session.id),
            format!("Run `boxr stop {}` to end it", session.id),
        ],
    );
    toon.render()
}

fn accounts(action: AccountAction, home: &Path, config: &Config) -> Result<i32> {
    match action {
        AccountAction::Add(args) => account_add(&args, home, config),
        AccountAction::List => account_list(home),
        AccountAction::Remove(args) => account_remove(&args, home, config),
    }
}

fn account_add(args: &AccountAdd, home: &Path, config: &Config) -> Result<i32> {
    let harness_id = resolve(
        "harness",
        args.harness.clone(),
        config.defaults.harness.clone(),
        home,
    )?;
    let adapter = adapter_for(&harness_id)?;
    if adapter.config_dir_env().is_none() {
        return Err(Fail::usage(
            format!("harness `{harness_id}` cannot isolate its config directory"),
            vec!["Use a harness boxr can point at a profile directory".to_string()],
        )
        .into());
    }

    let (dir, existed) = account::add(adapter.as_ref(), home, &args.name)?;

    let mut toon = Toon::new();
    toon.section("account")
        .field("harness", &harness_id)
        .field("name", &args.name)
        .field("dir", &dir.display().to_string())
        .field("status", if existed { "updated" } else { "created" });
    toon.list(
        "help",
        &[
            format!(
                "Run `boxr --harness {harness_id} --account {} \"<prompt>\"` to launch a session with this profile",
                args.name
            ),
            "Run `boxr account list` to see the profiles that exist".to_string(),
        ],
    );
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn account_list(home: &Path) -> Result<i32> {
    let profiles = account::list(home)?;
    let rows: Vec<Vec<String>> = profiles
        .iter()
        .map(|profile| {
            vec![
                profile.harness.clone(),
                profile.name.clone(),
                profile.dir.display().to_string(),
            ]
        })
        .collect();
    let mut toon = Toon::new();
    toon.table(
        "accounts",
        &["harness", "name", "dir"],
        &rows,
        &[Kind::Text, Kind::Text, Kind::Text],
    );
    let help = if profiles.is_empty() {
        vec!["Run `boxr account add --harness claude --name work` to create a profile".to_string()]
    } else {
        vec![
            "Run `boxr --harness claude --account <name> \"<prompt>\"` to launch with a profile"
                .to_string(),
            "Run `boxr account remove --harness <h> --name <n> --yes` to delete one".to_string(),
        ]
    };
    toon.list("help", &help);
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn account_remove(args: &AccountRemove, home: &Path, config: &Config) -> Result<i32> {
    let harness_id = resolve(
        "harness",
        args.harness.clone(),
        config.defaults.harness.clone(),
        home,
    )?;
    adapter_for(&harness_id)?;
    if !args.yes {
        return Err(Fail::usage(
            format!("removing account `{}` needs confirmation", args.name),
            vec![
                format!(
                    "Run `boxr account remove --harness {harness_id} --name {} --yes` to delete it",
                    args.name
                ),
                "Run `boxr account list` to see the profiles that exist".to_string(),
            ],
        )
        .into());
    }
    let dir = account::remove(home, &harness_id, &args.name)?;

    let mut toon = Toon::new();
    toon.section("account")
        .field("harness", &harness_id)
        .field("name", &args.name)
        .field("dir", &dir.display().to_string())
        .field("status", "removed");
    toon.list(
        "help",
        &["Run `boxr account list` to see the remaining profiles".to_string()],
    );
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn adapter_for(harness_id: &str) -> Result<std::sync::Arc<dyn harness::Harness>> {
    harness::lookup(harness_id).ok_or_else(|| {
        Fail::usage(
            format!("unknown harness `{harness_id}`"),
            vec![format!(
                "Known harnesses: {}",
                harness::known_ids().join(", ")
            )],
        )
        .into()
    })
}

fn profile_for(
    adapter: &dyn harness::Harness,
    home: &Path,
    harness_id: &str,
    name: &str,
) -> Result<PathBuf> {
    if adapter.config_dir_env().is_none() {
        return Err(Fail::usage(
            format!("harness `{harness_id}` cannot isolate its config directory"),
            vec![format!(
                "Run `boxr --harness {harness_id} \"<prompt>\"` without `--account`"
            )],
        )
        .into());
    }
    account::resolve(home, harness_id, name)
}

fn supervise(id: &str) -> Result<i32> {
    let home = home::boxr_home()?;
    let session = Session::open(&home, id);
    let launch = detached::read_launch(&session)?.ok_or_else(|| {
        Fail::usage(
            format!("session {id} has no launch record to supervise"),
            vec!["Run `boxr ps` to list the sessions boxr is running".to_string()],
        )
    })?;
    let adapter = harness::lookup(&launch.harness).ok_or_else(|| {
        Fail::usage(
            format!("unknown harness `{}`", launch.harness),
            vec![format!(
                "Known harnesses: {}",
                harness::known_ids().join(", ")
            )],
        )
    })?;
    let account = launch.profile.clone();
    let profile = match &account {
        Some(name) => Some(profile_for(adapter.as_ref(), &home, &launch.harness, name)?),
        None => None,
    };
    let request = LaunchRequest {
        model: launch.model,
        effort: launch.effort,
        prompt: launch.prompt,
        cwd: launch.cwd,
        mode: LaunchMode::Fresh,
        kind: launch.kind,
        kind_source: launch.kind_source,
    };
    let launch = run::adopt(&adapter, &request, session)?;
    let report = run::headless(
        &adapter,
        &request,
        run::Account {
            name: account.as_deref(),
            dir: profile.as_deref(),
        },
        launch,
    )?;
    Ok(report::exit_code(&report))
}

fn ps() -> Result<i32> {
    let home = home::boxr_home()?;
    let mut ids = Vec::new();
    let mut rows = Vec::new();
    for id in Session::ids(&home)? {
        let Ok(state) = detached::state(&home, &id) else {
            continue;
        };
        if let State::Running(running) = state {
            let pid = running
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string());
            rows.push(format!(
                "{id} {} {} {pid} {} {}",
                running.launch.harness,
                running.launch.model,
                clock::iso8601(running.launch.started_millis as u128),
                running.steps
            ));
            ids.push(id);
        }
    }
    let mut toon = Toon::new();
    toon.section("ps").number("running", rows.len());
    toon.list("sessions", &rows);
    let help = match ids.first() {
        Some(id) => vec![
            format!("Run `boxr status {id}` to check on a session without waiting"),
            format!("Run `boxr wait {id}` to block until it finishes"),
        ],
        None => vec![
            "Run `boxr --detach --harness <h> --model <m> \"<prompt>\"` to start a background session"
                .to_string(),
        ],
    };
    toon.list("help", &help);
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn status(id: &str) -> Result<i32> {
    let home = home::boxr_home()?;
    let session = Session::open(&home, id);
    match detached::state(&home, id)? {
        State::Running(running) => print!("{}", render_running(&session, &running)),
        State::Finished(report) => print!("{}", report::render(&report, &session)),
    }
    Ok(EXIT_OK)
}

fn render_running(session: &Session, running: &detached::Running) -> String {
    let mut toon = Toon::new();
    toon.section("session")
        .field("id", &session.id)
        .field("status", "running")
        .field("harness", &running.launch.harness)
        .field("model", &running.launch.model)
        .field(
            "effort",
            running
                .launch
                .effort
                .as_deref()
                .unwrap_or("harness-default"),
        )
        .field(
            "started",
            &clock::iso8601(running.launch.started_millis as u128),
        )
        .number("steps", running.steps);
    if let Some(pid) = running.pid {
        toon.number("pid", pid);
    }
    toon.list(
        "help",
        &[
            format!("Run `boxr wait {}` to block until it finishes", session.id),
            format!("Run `boxr tail {}` to stream its steps", session.id),
            format!("Run `boxr stop {}` to end it", session.id),
        ],
    );
    toon.render()
}

fn wait(id: &str, timeout: Option<u64>) -> Result<i32> {
    let home = home::boxr_home()?;
    let session = Session::open(&home, id);
    let deadline = timeout.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    loop {
        if let Some(report) = detached::settled(&home, id)? {
            print!("{}", report::render(&report, &session));
            return Ok(report::exit_code(&report));
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Fail::wait_timeout(
                format!("session {id} is still running and the wait timed out"),
                vec![format!("Run `boxr status {id}` to check on it")],
            )
            .into());
        }
        std::thread::sleep(detached::POLL);
    }
}

fn tail(id: &str) -> Result<i32> {
    let home = home::boxr_home()?;
    let session = Session::open(&home, id);
    detached::state(&home, id)?;
    let path = session.normalized_path();
    let mut stdout = std::io::stdout();
    let mut source = None;
    let mut pending = Vec::new();
    let mut finished = false;
    let mut ticks = 0u32;
    loop {
        if source.is_none() {
            source = std::fs::File::open(&path).ok();
        }
        let mut read_bytes = 0;
        if let Some(file) = source.as_mut() {
            read_bytes = file
                .read_to_end(&mut pending)
                .with_context(|| format!("reading {}", path.display()))?;
            if let Some(last) = pending.iter().rposition(|byte| *byte == b'\n') {
                stdout.write_all(&pending[..=last])?;
                stdout.flush()?;
                pending.drain(..=last);
            }
        }
        if finished && read_bytes == 0 {
            if !pending.is_empty() {
                stdout.write_all(&pending)?;
                stdout.flush()?;
            }
            return Ok(EXIT_OK);
        }
        ticks += 1;
        if !finished && ticks % 2 == 0 {
            finished = !detached::running(&session)?;
        }
        std::thread::sleep(detached::POLL);
    }
}

fn stop(id: &str) -> Result<i32> {
    let home = home::boxr_home()?;
    let session = Session::open(&home, id);
    let mut action = "already-finished";
    if let State::Running(running) = detached::state(&home, id)? {
        action = "stopped";
        detached::request_stop(&session)?;
        if detached::finish(&home, id, STOP_BUDGET).is_err() {
            let pid = running.pid.ok_or_else(|| {
                anyhow::anyhow!("session {id} is still starting and has no supervisor to stop")
            })?;
            detached::hard_kill(pid);
            detached::finish(&home, id, KILL_BUDGET)?;
        }
    }
    let summary = ledger::read_summary(&home, id).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec![format!("Run `boxr status {id}` to check on the session")],
        )
    })?;
    print!("{}", render_stopped(&session, &summary, action));
    Ok(EXIT_OK)
}

fn render_stopped(session: &Session, summary: &Summary, action: &str) -> String {
    let mut toon = Toon::new();
    toon.section("stop")
        .field("id", &session.id)
        .field("action", action);
    summarize(&mut toon, summary);
    toon.list(
        "help",
        &[
            format!("Run `boxr show {}` to read the session summary", session.id),
            format!(
                "Run `boxr export --atif {}` to write a standard ATIF trajectory",
                session.id
            ),
        ],
    );
    toon.render()
}

fn resume(id: &str, prompt: &str) -> Result<i32> {
    let home = home::boxr_home()?;
    if let State::Running(_) = detached::state(&home, id)? {
        return Err(Fail::usage(
            format!("session {id} is still running"),
            vec![
                format!("Run `boxr wait {id}` to block until it finishes"),
                format!("Run `boxr stop {id}` to end it, then resume the session"),
            ],
        )
        .into());
    }
    let summary = ledger::read_summary(&home, id)?;
    let harness_session_id = summary.harness_session_id.clone().ok_or_else(|| {
        Fail::usage(
            format!("session {id} recorded no harness session id to resume"),
            vec![format!(
                "Run `boxr show {id}` to check what the session recorded"
            )],
        )
    })?;
    let adapter = harness::lookup(&summary.harness).ok_or_else(|| {
        Fail::usage(
            format!("unknown harness `{}`", summary.harness),
            vec![format!(
                "Known harnesses: {}",
                harness::known_ids().join(", ")
            )],
        )
    })?;
    let account = summary.profile.clone();
    let profile = match &account {
        Some(name) => Some(profile_for(
            adapter.as_ref(),
            &home,
            &summary.harness,
            name,
        )?),
        None => None,
    };
    let parent = run::harness_session_of(&Session::open(&home, id));
    let from_bytes = run::transcript_size(
        adapter.as_ref(),
        &parent,
        &harness_session_id,
        profile.as_deref(),
    )
    .map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec![
                format!("Resuming needs the harness transcript of {harness_session_id}"),
                format!("Run `boxr show {id}` to check the session"),
            ],
        )
    })?;
    let request = LaunchRequest {
        model: summary.model.clone(),
        effort: summary.effort.clone(),
        prompt: prompt.to_string(),
        cwd: resume_cwd(&home, id)?,
        mode: LaunchMode::Resume { harness_session_id },
        kind: summary.kind.clone(),
        kind_source: summary.kind_source.clone(),
    };
    let continuation = run::Continuation {
        parent: id.to_string(),
        profile: account.clone(),
        from_bytes,
    };
    let launch = run::resume(&adapter, &request, &continuation, &home)?;
    let report = run::headless(
        &adapter,
        &request,
        run::Account {
            name: account.as_deref(),
            dir: profile.as_deref(),
        },
        launch,
    )?;
    print!(
        "{}",
        report::render(&report, &Session::open(&home, &report.id))
    );
    Ok(report::exit_code(&report))
}

fn resume_cwd(home: &Path, id: &str) -> Result<PathBuf> {
    let session = Session::open(home, id);
    if let Some(launch) = detached::read_launch(&session)? {
        return Ok(launch.cwd);
    }
    std::env::current_dir().context("reading the current directory")
}

fn show(id: &str) -> Result<i32> {
    let home = home::boxr_home()?;
    let summary = ledger::read_summary(&home, id).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec!["Run `boxr show <id>` with an id printed by a boxr launch".to_string()],
        )
    })?;
    let session = Session::open(&home, id);
    let mut toon = Toon::new();
    summarize(&mut toon, &summary);
    toon.section("files")
        .field(
            "normalized",
            &session.normalized_path().display().to_string(),
        )
        .field("raw", &session.raw_dir().display().to_string());
    toon.list(
        "help",
        &[
            format!("Run `boxr export --atif {id}` to write a standard ATIF trajectory"),
            format!("Run `boxr resume {id} \"<prompt>\"` to continue the session"),
        ],
    );
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn summarize(toon: &mut Toon, summary: &Summary) {
    toon.section("session")
        .field("id", &summary.id)
        .field("status", &summary.status)
        .field("harness", &summary.harness)
        .field(
            "harnessSessionId",
            summary.harness_session_id.as_deref().unwrap_or("unknown"),
        )
        .field("model", &summary.model)
        .field(
            "effort",
            summary.effort.as_deref().unwrap_or("harness-default"),
        )
        .field("profile", summary.profile.as_deref().unwrap_or("default"))
        .field("mode", &summary.mode)
        .field("kind", summary.kind.as_deref().unwrap_or("unclassified"));
    if let Some(source) = &summary.kind_source {
        toon.field("kindSource", source);
    }
    if let Some(parent) = &summary.resumed_from {
        toon.field("resumedFrom", parent);
    }
    if let Some(verdict) = &summary.verdict {
        toon.field("verdict", verdict);
    }
    if let Some(note) = &summary.verdict_note {
        toon.field("verdictNote", &one_line(note, MESSAGE_LIMIT));
    }
    if summary.limit_hit {
        toon.flag("limitHit", true);
    }
    if let Some(error) = &summary.error {
        toon.field("error", &one_line(error, MESSAGE_LIMIT));
    }
    if let Some(evidence) = &summary.git {
        toon.section("git")
            .field("repo", &evidence.repo.display().to_string())
            .field("branch", evidence.branch.as_deref().unwrap_or("none"))
            .number("commits", evidence.commits.len())
            .number("files", evidence.files.len());
        if let Some(reverted) = &evidence.reverted {
            toon.number("reverted", reverted.len());
        }
    }
    toon.field("start", &summary.start)
        .field("end", &summary.end)
        .number("durationMs", summary.duration_ms)
        .number("exitCode", summary.exit_code)
        .number("steps", summary.steps)
        .number("promptTokens", summary.prompt_tokens)
        .number("completionTokens", summary.completion_tokens)
        .number("cachedTokens", summary.cached_tokens);
}

const VERDICTS: &[&str] = &["success", "partial", "failed"];

fn outcome(
    check_reverted: bool,
    id: Option<String>,
    verdict: Option<String>,
    note: Option<String>,
) -> Result<i32> {
    let home = home::boxr_home()?;
    let id = id.ok_or_else(|| {
        anyhow::Error::from(Fail::usage(
            "no session id given",
            vec!["Run `boxr outcome <id> success|partial|failed --note \"...\"`".to_string()],
        ))
    })?;
    if check_reverted {
        if verdict.is_some() || note.is_some() {
            return Err(Fail::usage(
                "--check-reverted takes no verdict or note",
                vec![format!("Run `boxr outcome --check-reverted {id}`")],
            )
            .into());
        }
        return check_commits(&home, &id);
    }
    let verdict = verdict.ok_or_else(|| {
        anyhow::Error::from(Fail::usage(
            "no verdict given",
            vec![
                format!("Run `boxr outcome {id} success|partial|failed`"),
                format!("Valid verdicts: {}", VERDICTS.join(", ")),
            ],
        ))
    })?;
    if !VERDICTS.contains(&verdict.as_str()) {
        return Err(Fail::usage(
            format!("unknown verdict `{verdict}`"),
            vec![format!("Valid verdicts: {}", VERDICTS.join(", "))],
        )
        .into());
    }
    let summary = ledger::read_summary(&home, &id).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec!["Run `boxr show <id>` with an id printed by a boxr launch".to_string()],
        )
    })?;
    let updated = Summary {
        verdict: Some(verdict.clone()),
        verdict_note: note.clone().or(summary.verdict_note.clone()),
        ..summary
    };
    ledger::rewrite_summary(&home.join(ledger::SUMMARY_FILE), &updated).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec![format!("Run `boxr show {id}` to check the session")],
        )
    })?;

    let mut toon = Toon::new();
    toon.section("outcome")
        .field("id", &id)
        .field("verdict", &verdict);
    if let Some(note) = &updated.verdict_note {
        toon.field("note", &one_line(note, MESSAGE_LIMIT));
    }
    toon.list(
        "help",
        &[
            format!("Run `boxr show {id}` to read the session summary"),
            "Run `boxr stats --by verdict --since 7d` to group sessions by verdict".to_string(),
        ],
    );
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn check_commits(home: &Path, id: &str) -> Result<i32> {
    let summary = ledger::read_summary(home, id).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec!["Run `boxr show <id>` with an id printed by a boxr launch".to_string()],
        )
    })?;
    let mut evidence = summary.git.clone().ok_or_else(|| {
        Fail::usage(
            format!("session {id} recorded no git evidence"),
            vec![format!(
                "Run `boxr show {id}` to check what the session recorded"
            )],
        )
    })?;
    let branch = evidence.branch.clone().ok_or_else(|| {
        Fail::usage(
            format!("session {id} recorded no branch to check against"),
            vec![format!(
                "Run `boxr show {id}` to check what the session recorded"
            )],
        )
    })?;
    let commits = evidence.commits.clone();
    let mut reverted = Vec::new();
    if !commits.is_empty() {
        if !git::branch_exists(&evidence.repo, &branch).map_err(outcome_git_error(id))? {
            reverted = commits.clone();
        } else {
            for commit in &commits {
                if !git::reachable_from(&evidence.repo, commit, &branch)
                    .map_err(outcome_git_error(id))?
                {
                    reverted.push(commit.clone());
                }
            }
        }
    }
    let survived: Vec<String> = commits
        .iter()
        .filter(|commit| !reverted.contains(commit))
        .cloned()
        .collect();
    evidence.reverted = Some(reverted.clone());
    let checked = Summary {
        git: Some(evidence),
        ..summary
    };
    ledger::rewrite_summary(&home.join(ledger::SUMMARY_FILE), &checked).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec![format!("Run `boxr show {id}` to check the session")],
        )
    })?;

    let mut toon = Toon::new();
    toon.section("reverts")
        .field("id", id)
        .field("branch", &branch)
        .number("commits", commits.len());
    toon.list("reverted", &reverted);
    toon.list("survived", &survived);
    toon.list(
        "help",
        &[
            format!("Run `boxr show {id}` to read the session summary"),
            "Run `boxr stats --by verdict --since 7d` to group sessions by verdict".to_string(),
        ],
    );
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn outcome_git_error(id: &str) -> impl Fn(anyhow::Error) -> anyhow::Error {
    let id = id.to_string();
    move |error: anyhow::Error| {
        Fail::usage(
            format!("cannot check the commits of session {id}: {error:#}"),
            vec![format!("Run `boxr show {id}` to check the session")],
        )
        .into()
    }
}

fn stats(home: &Path, by: &str, since: &str) -> Result<i32> {
    print!(
        "{}",
        stats::render(home, by, since).map_err(|error| {
            Fail::usage(
                format!("{error:#}"),
                vec!["Run `boxr stats --by model,kind --since 7d`".to_string()],
            )
        })?
    );
    Ok(EXIT_OK)
}

fn declared_kind(kind: Option<&str>, config: &Config) -> Result<Option<String>> {
    let Some(kind) = kind else {
        return Ok(None);
    };
    let core = [
        "build", "fix", "research", "plan", "review", "chore", "docs",
    ];
    if core.contains(&kind) || config.kinds.contains_key(kind) {
        return Ok(Some(kind.to_string()));
    }
    let mut valid: Vec<&str> = core.to_vec();
    valid.extend(config.kinds.keys().map(String::as_str));
    valid.sort_unstable();
    Err(Fail::usage(
        format!("unknown kind `{kind}`"),
        vec![format!("Valid kinds: {}", valid.join(", "))],
    )
    .into())
}

fn export(atif: bool, id: &str) -> Result<i32> {
    if !atif {
        return Err(anyhow::Error::from(Fail::usage(
            "no export format given",
            vec![format!("Run `boxr export --atif {id}`")],
        )));
    }
    let home = home::boxr_home()?;
    let summary = ledger::read_summary(&home, id).map_err(|error| {
        Fail::usage(
            format!("{error:#}"),
            vec!["Run `boxr export --atif <id>` with an id printed by a boxr launch".to_string()],
        )
    })?;
    let session = Session::open(&home, id);
    let document = ledger::trajectory(&session.normalized_path(), &summary)?;
    if document.steps.is_empty() {
        return Err(anyhow::Error::from(Fail::usage(
            format!("session {id} has no steps, and an ATIF trajectory needs at least one"),
            vec![format!("Run `boxr show {id}` to see how the session ended")],
        )));
    }
    let target = session.trajectory_path();
    ledger::write_trajectory(&target, &document)?;

    let mut toon = Toon::new();
    toon.section("export")
        .field("id", &summary.id)
        .field("format", "atif")
        .field("schemaVersion", atif::SCHEMA_VERSION)
        .number("steps", document.steps.len())
        .field("path", &target.display().to_string());
    toon.list(
        "help",
        &[format!("Read the trajectory at {}", target.display())],
    );
    print!("{}", toon.render());
    Ok(EXIT_OK)
}

fn resolve(
    name: &str,
    flag: Option<String>,
    default: Option<String>,
    home: &std::path::Path,
) -> Result<String> {
    flag.or(default).ok_or_else(|| {
        anyhow::Error::from(Fail::usage(
            format!("no {name} given and no default configured"),
            vec![
                format!("Pass `--{name} <value>`"),
                format!("Or set defaults.{name} in {}", Config::path(home).display()),
            ],
        ))
    })
}

fn report_error(error: &anyhow::Error) -> i32 {
    let (code, message, help) = match error.downcast_ref::<Fail>() {
        Some(fail) => (fail.code, fail.message.clone(), fail.help.clone()),
        None => (
            EXIT_INTERNAL,
            error
                .chain()
                .map(|cause| cause.to_string())
                .collect::<Vec<_>>()
                .join(": "),
            vec!["Run the same command again with `--help` to check the arguments".to_string()],
        ),
    };
    let mut toon = Toon::new();
    toon.section("error")
        .field("message", &one_line(&message, MESSAGE_LIMIT))
        .number("exitCode", code);
    toon.list("help", &help);
    eprint!("{}", toon.render());
    code
}
