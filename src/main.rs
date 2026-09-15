mod config;
mod fail;
mod harness;
mod home;
mod output;
mod run;
mod session;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use fail::{Fail, EXIT_INTERNAL, EXIT_OK, EXIT_SESSION_FAILED};
use harness::{LaunchRequest, Mode};
use output::{one_line, Toon};
use session::Session;
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
    #[arg(long, value_name = "HARNESS")]
    harness: Option<String>,

    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    #[arg(long, value_name = "EFFORT")]
    effort: Option<String>,

    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,
}

fn main() -> ExitCode {
    match launch() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => ExitCode::from(report(&error) as u8),
    }
}

fn launch() -> Result<i32> {
    let cli = Cli::parse();
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
        mode: Mode::Headless,
        model,
        effort,
        prompt,
        cwd,
    };

    let session = Session::create(&home)?;
    let outcome =
        run::headless(adapter.as_ref(), &request, &session).map_err(|error| {
            match error.downcast::<Fail>() {
                Ok(fail) => anyhow::Error::from(fail),
                Err(error) => {
                    let message = error.to_string();
                    if message.contains("was not found on PATH") {
                        anyhow::Error::from(Fail::harness_unavailable(
                            message,
                            vec![format!(
                                "Install {} or put its executable on PATH",
                                adapter.id()
                            )],
                        ))
                    } else {
                        error
                    }
                }
            }
        })?;

    print!("{}", render(&session, adapter.id(), &request, &outcome));

    Ok(if outcome.succeeded() {
        EXIT_OK
    } else {
        EXIT_SESSION_FAILED
    })
}

fn render(
    session: &Session,
    harness_id: &str,
    request: &LaunchRequest,
    outcome: &run::Outcome,
) -> String {
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
        .field("durationMs", &outcome.duration_ms.to_string())
        .field("exitCode", &outcome.exit_code.to_string())
        .field(
            "message",
            &one_line(
                outcome.final_message.as_deref().unwrap_or(""),
                MESSAGE_LIMIT,
            ),
        );
    toon.section("raw")
        .field("dir", &session.raw_dir().display().to_string())
        .field("stream", &session.stream_path().display().to_string())
        .field(
            "transcript",
            &outcome
                .transcript
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
        );

    let help = if outcome.succeeded() {
        vec![
            format!(
                "Read the raw transcript at {}",
                session.transcript_path().display()
            ),
            "Run `boxr --harness claude --model <m> \"<prompt>\"` to launch another session"
                .to_string(),
        ]
    } else {
        vec![
            format!(
                "Read {} for the harness error output",
                session.stderr_path().display()
            ),
            format!("Read the raw stream at {}", session.stream_path().display()),
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
        .field("exitCode", &code.to_string());
    toon.list("help", &help);
    eprint!("{}", toon.render());
    code
}
