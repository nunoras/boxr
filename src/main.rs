mod config;
mod fail;
mod harness;
mod home;
mod output;
mod run;
mod session;
mod skill;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use config::Config;
use fail::{Fail, EXIT_INTERNAL, EXIT_LEDGER_FAILED, EXIT_OK, EXIT_SESSION_FAILED};
use harness::LaunchRequest;
use output::{one_line, Toon};
use run::Ledger;
use std::process::ExitCode;

const MESSAGE_LIMIT: usize = 200;

#[derive(Parser, Debug)]
#[command(
    name = "boxr",
    version,
    about = "Launch coding agents and record every session in a local ledger",
    disable_help_subcommand = true
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

    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
}

#[derive(Subcommand, Debug)]
enum SkillAction {
    Install(SkillInstall),
}

#[derive(Args, Debug)]
struct SkillInstall {
    #[arg(long, value_name = "HARNESS")]
    harness: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => ExitCode::from(report(&error) as u8),
    }
}

fn run() -> Result<i32> {
    let cli = Cli::parse();
    if let Some(Command::Skill { action }) = &cli.command {
        return match action {
            SkillAction::Install(args) => skill_install(&args.harness),
        };
    }
    launch(cli)
}

fn skill_install(harness: &str) -> Result<i32> {
    let targets: Vec<&'static str> = if harness == "all" {
        skill::ids().collect()
    } else {
        match skill::ids().find(|id| *id == harness) {
            Some(id) => vec![id],
            None => {
                return Err(Fail::usage(
                    format!("unknown harness `{harness}`"),
                    vec![format!(
                        "Choose one of: {}, all",
                        skill::ids().collect::<Vec<_>>().join(", ")
                    )],
                )
                .into())
            }
        }
    };

    let mut installed = Vec::new();
    for id in targets {
        installed.push(skill::install(id)?);
    }

    print!("{}", render_skill_install(&installed));
    Ok(EXIT_OK)
}

fn render_skill_install(installed: &[skill::Installed]) -> String {
    let mut toon = Toon::new();
    toon.section("skill")
        .field("name", skill::SKILL_NAME)
        .field("action", "install")
        .number("harnesses", installed.len());
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

fn launch(cli: Cli) -> Result<i32> {
    let prompt = cli.prompt.clone().ok_or_else(|| {
        Fail::usage(
            "no prompt given",
            vec!["Run `boxr --harness claude --model <m> \"<prompt>\"`".to_string()],
        )
    })?;

    let home = home::ensure_home()?;
    let config = Config::load(&home)?;

    let harness_id = resolve(
        "harness",
        cli.harness,
        config.defaults.harness.clone(),
        &home,
    )?;
    let model = resolve("model", cli.model, config.defaults.model.clone(), &home)?;
    let effort = cli.effort.or(config.defaults.effort.clone());

    let adapter = harness::lookup(&harness_id).ok_or_else(|| {
        Fail::usage(
            format!("unknown harness `{harness_id}`"),
            vec![format!(
                "Known harnesses: {}",
                harness::known_ids().join(", ")
            )],
        )
    })?;

    let cwd = std::env::current_dir().context("reading the current directory")?;
    let request = LaunchRequest {
        model,
        effort,
        prompt,
        cwd,
    };

    let outcome = run::headless(adapter.as_ref(), &request, &home)?;

    print!("{}", render(adapter.id(), &request, &outcome));

    Ok(match (outcome.succeeded(), &outcome.ledger) {
        (false, _) => EXIT_SESSION_FAILED,
        (true, Ledger::Failed(_)) => EXIT_LEDGER_FAILED,
        (true, Ledger::Recorded(_)) => EXIT_OK,
    })
}

fn render(harness_id: &str, request: &LaunchRequest, outcome: &run::Outcome) -> String {
    let session = &outcome.session;
    let mut toon = Toon::new();
    toon.section("session")
        .field("id", &session.id)
        .field("status", if outcome.succeeded() { "ok" } else { "failed" })
        .field("harness", harness_id)
        .field("model", &request.model)
        .field(
            "effort",
            request.effort.as_deref().unwrap_or("harness-default"),
        )
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
        vec![
            record_line,
            "Run `boxr --harness claude --model <m> \"<prompt>\"` to launch another session"
                .to_string(),
        ]
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
