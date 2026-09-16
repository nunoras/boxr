mod account;
mod atif;
mod clock;
mod config;
mod detached;
mod fail;
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

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use config::Config;
use detached::{LaunchFile, State};
use fail::{Fail, EXIT_INTERNAL, EXIT_OK};
use harness::LaunchRequest;
use ledger::Summary;
use output::{one_line, Toon, MESSAGE_LIMIT};
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
            account: account.map(str::to_string),
            started_millis: clock::now_millis() as u64,
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
    toon.table("accounts", &["harness", "name", "dir"], &rows);
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
    let account = launch.account.clone();
    let profile = match &account {
        Some(name) => Some(profile_for(adapter.as_ref(), &home, &launch.harness, name)?),
        None => None,
    };
    let request = LaunchRequest {
        model: launch.model,
        effort: launch.effort,
        prompt: launch.prompt,
        cwd: launch.cwd,
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
        &[format!(
            "Run `boxr export --atif {id}` to write a standard ATIF trajectory"
        )],
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
        .field("start", &summary.start)
        .field("end", &summary.end)
        .number("durationMs", summary.duration_ms)
        .number("exitCode", summary.exit_code)
        .number("steps", summary.steps)
        .number("promptTokens", summary.prompt_tokens)
        .number("completionTokens", summary.completion_tokens)
        .number("cachedTokens", summary.cached_tokens);
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
