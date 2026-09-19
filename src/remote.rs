use crate::fail::Fail;
use crate::output::Toon;
use crate::run;
use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;
use std::process::Command;

const LOCAL_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct RemoteLaunch {
    pub host: String,
    pub harness: String,
    pub model: String,
    pub effort: Option<String>,
    pub account: Option<String>,
    pub kind: Option<String>,
    pub prompt: String,
}

pub fn launch(request: &RemoteLaunch) -> Result<i32> {
    probe_version(&request.host)?;
    let output = run_ssh(
        &request.host,
        &build_detach_args(request),
        "launching a remote boxr session",
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail =
            first_nonempty(stderr.trim(), stdout.trim()).unwrap_or("the remote boxr launch failed");
        return Err(Fail::usage(
            format!("remote launch on `{}` failed: {detail}", request.host),
            vec![
                format!("ssh to `{}` and run `boxr --version`", request.host),
                "Confirm the remote host has the same boxr version on PATH".to_string(),
            ],
        )
        .into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let id = session_id_of(&stdout).ok_or_else(|| {
        Fail::usage(
            format!(
                "remote boxr on `{}` did not print a session id",
                request.host
            ),
            vec!["Run the same launch on the remote host and inspect its output".to_string()],
        )
    })?;
    print!("{}", render_remote(&request.host, &id, request));
    Ok(crate::fail::EXIT_OK)
}

fn probe_version(host: &str) -> Result<()> {
    let output = run_ssh(
        host,
        &["boxr".into(), "--version".into()],
        "probing remote boxr",
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let detail = first_nonempty(stderr.trim(), stdout.trim())
            .unwrap_or("boxr is missing or failed on the remote host");
        return Err(Fail::usage(
            format!("remote boxr on `{host}` is unavailable: {detail}"),
            vec![
                format!("Install boxr {LOCAL_VERSION} on `{host}` and put it on PATH"),
                format!("ssh to `{host}` and run `boxr --version`"),
            ],
        )
        .into());
    }
    let remote = parse_version(&stdout).ok_or_else(|| {
        Fail::usage(
            format!("remote boxr on `{host}` did not report a version"),
            vec![format!("ssh to `{host}` and run `boxr --version`")],
        )
    })?;
    if remote != LOCAL_VERSION {
        return Err(Fail::usage(
            format!("remote boxr on `{host}` is {remote}, local is {LOCAL_VERSION}"),
            vec![
                format!("Install boxr {LOCAL_VERSION} on `{host}`"),
                "Remote launches require the same boxr version on both hosts".to_string(),
            ],
        )
        .into());
    }
    Ok(())
}

fn build_detach_args(request: &RemoteLaunch) -> Vec<String> {
    let mut args = vec![
        "boxr".to_string(),
        "--detach".to_string(),
        "--harness".to_string(),
        request.harness.clone(),
        "--model".to_string(),
        request.model.clone(),
    ];
    if let Some(effort) = &request.effort {
        args.push("--effort".to_string());
        args.push(effort.clone());
    }
    if let Some(account) = &request.account {
        args.push("--account".to_string());
        args.push(account.clone());
    }
    if let Some(kind) = &request.kind {
        args.push("--kind".to_string());
        args.push(kind.clone());
    }
    args.push(request.prompt.clone());
    args
}

fn run_ssh(host: &str, remote_args: &[String], context: &str) -> Result<std::process::Output> {
    let program = ssh_program()?;
    let command = shell_join(remote_args);
    Command::new(&program)
        .arg(host)
        .arg(command)
        .output()
        .with_context(|| format!("{context} over ssh via {}", program.display()))
}

fn ssh_program() -> Result<PathBuf> {
    if let Some(value) = env::var_os("BOXR_TEST_SSH") {
        return Ok(PathBuf::from(value));
    }
    run::locate("ssh").ok_or_else(|| {
        anyhow::Error::from(Fail::usage(
            "ssh is not on PATH",
            vec![
                "Install OpenSSH and put `ssh` on PATH".to_string(),
                "Remote launches need ssh access to the host".to_string(),
            ],
        ))
    })
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '=' | ':' | '+')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_version(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("boxr ") {
            let version = rest.split_whitespace().next()?.to_string();
            if !version.is_empty() {
                return Some(version);
            }
        }
        if line.chars().all(|ch| ch.is_ascii_digit() || ch == '.') && !line.is_empty() {
            return Some(line.to_string());
        }
    }
    None
}

fn session_id_of(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        line.trim()
            .strip_prefix("id: ")
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
    })
}

fn first_nonempty<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    if !a.is_empty() {
        Some(a)
    } else if !b.is_empty() {
        Some(b)
    } else {
        None
    }
}

fn render_remote(host: &str, id: &str, request: &RemoteLaunch) -> String {
    let mut toon = Toon::new();
    toon.section("session")
        .field("id", id)
        .field("status", "running")
        .field("remote", host)
        .field("harness", &request.harness)
        .field("model", &request.model)
        .field(
            "effort",
            request.effort.as_deref().unwrap_or("harness-default"),
        );
    toon.list(
        "help",
        &[
            format!("The session lives on `{host}`; follow it there or through `boxr serve`"),
            format!("ssh to `{host}` and run `boxr status {id}`"),
            format!("ssh to `{host}` and run `boxr wait {id}`"),
        ],
    );
    toon.render()
}
