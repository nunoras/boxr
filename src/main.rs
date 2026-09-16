mod account;
mod atif;
mod clock;
mod config;
mod fail;
mod harness;
mod home;
mod ledger;
mod output;
mod run;
mod session;
mod skill;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use config::Config;
use fail::{Fail, EXIT_INTERNAL, EXIT_LEDGER_FAILED, EXIT_OK, EXIT_SESSION_FAILED};
use harness::LaunchRequest;
use ledger::Summary;
use output::{one_line, Toon};
use run::Ledger;
use session::Session;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MESSAGE_LIMIT: usize = 200;

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
        Err(error) => ExitCode::from(report(&error) as u8),
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

    let outcome = run::headless(&adapter, &request, profile.as_deref(), home)?;

    print!(
        "{}",
        render(adapter.id(), &request, &outcome, account.as_deref())
    );

    let ledger_failed = matches!(outcome.ledger, Ledger::Failed(_))
        || outcome.tally.error.is_some()
        || outcome.summary_error.is_some();
    Ok(match (outcome.succeeded(), ledger_failed) {
        (false, _) => EXIT_SESSION_FAILED,
        (true, true) => EXIT_LEDGER_FAILED,
        (true, false) => EXIT_OK,
    })
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

fn render(
    harness_id: &str,
    request: &LaunchRequest,
    outcome: &run::Outcome,
    account: Option<&str>,
) -> String {
    let session = &outcome.session;
    let mut toon = Toon::new();
    toon.section("session")
        .field("id", &session.id)
        .field("status", &outcome.summary.status)
        .field("harness", harness_id)
        .field("model", &request.model)
        .field(
            "effort",
            request.effort.as_deref().unwrap_or("harness-default"),
        )
        .field("account", account.unwrap_or("harness-default"))
        .field(
            "harnessSessionId",
            outcome.harness_session_id.as_deref().unwrap_or("unknown"),
        )
        .number("durationMs", outcome.duration_ms)
        .number("exitCode", outcome.exit_code)
        .field(
            "message",
            &one_line(
                outcome.final_message.as_deref().unwrap_or(""),
                MESSAGE_LIMIT,
            ),
        );
    let ledger = toon
        .section("ledger")
        .field(
            "normalized",
            &session.normalized_path().display().to_string(),
        )
        .number("steps", outcome.tally.steps)
        .number("promptTokens", outcome.tally.prompt_tokens)
        .number("completionTokens", outcome.tally.completion_tokens)
        .number("cachedTokens", outcome.tally.cached_tokens)
        .field("summary", &outcome.summary_path.display().to_string());
    if let Some(error) = &outcome.tally.error {
        ledger.field("captureError", &one_line(error, MESSAGE_LIMIT));
    }
    if let Some(error) = &outcome.summary_error {
        ledger.field("summaryError", &one_line(error, MESSAGE_LIMIT));
    }
    let raw = toon
        .section("raw")
        .field("dir", &session.raw_dir().display().to_string())
        .field("stream", &session.stream_path().display().to_string());
    let record_line = match &outcome.ledger {
        Ledger::Recorded(transcript) => {
            raw.field("ledger", "recorded")
                .field("transcript", &transcript.display().to_string());
            format!("Read the raw transcript at {}", transcript.display())
        }
        Ledger::Failed(reason) => {
            raw.field("ledger", "failed")
                .field("ledgerError", &one_line(reason, MESSAGE_LIMIT));
            format!("Read the raw stream at {}", session.stream_path().display())
        }
    };

    let help = if outcome.succeeded() {
        let mut lines = vec![record_line];
        if outcome.summary_error.is_none() {
            lines.push(format!(
                "Run `boxr show {}` to read the session summary",
                session.id
            ));
        }
        lines
    } else {
        vec![
            format!(
                "Read {} for the harness error output",
                session.stderr_path().display()
            ),
            record_line,
        ]
    };
    toon.list("help", &help);
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

fn report(error: &anyhow::Error) -> i32 {
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
