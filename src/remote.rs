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
    pub dir: Option<String>,
    pub harness: String,
    pub model: String,
    pub effort: Option<String>,
    pub account: Option<String>,
    pub kind: Option<String>,
    pub prompt: String,
    pub exact_version: bool,
}

struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    fn same_release(&self, other: &Version) -> bool {
        self.major == other.major && self.minor == other.minor
    }
}

pub fn launch(request: &RemoteLaunch) -> Result<i32> {
    validate_dir(request)?;
    probe_version(&request.host, request.exact_version)?;
    let output = run_ssh(
        &request.host,
        &detach_command(request),
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

fn validate_dir(request: &RemoteLaunch) -> Result<()> {
    match request.dir.as_deref() {
        Some("") => Err(Fail::usage(
            "--remote-dir needs a path",
            vec![
                "Pass a directory that exists on the remote host, for example `--remote-dir /srv/app`"
                    .to_string(),
                "Drop --remote-dir to use the remote login directory".to_string(),
            ],
        )
        .into()),
        _ => Ok(()),
    }
}

fn probe_version(host: &str, exact: bool) -> Result<()> {
    let output = run_ssh(host, "boxr --version", "probing remote boxr")?;
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
    let remote_text = parse_version(&stdout).ok_or_else(|| {
        Fail::usage(
            format!("remote boxr on `{host}` did not report a version"),
            vec![format!("ssh to `{host}` and run `boxr --version`")],
        )
    })?;
    let remote = Version::parse(&remote_text).ok_or_else(|| {
        Fail::usage(
            format!("remote boxr on `{host}` reported an unreadable version `{remote_text}`"),
            vec![
                format!("ssh to `{host}` and run `boxr --version` to see the raw output"),
                format!("Install boxr {LOCAL_VERSION} on `{host}` and put it on PATH"),
            ],
        )
    })?;
    let local = Version::parse(LOCAL_VERSION).ok_or_else(|| {
        Fail::usage(
            format!("this boxr build reports an unreadable version `{LOCAL_VERSION}`"),
            vec!["Reinstall boxr from a released binary".to_string()],
        )
    })?;
    let matches = if exact {
        remote.major == local.major && remote.minor == local.minor && remote.patch == local.patch
    } else {
        remote.same_release(&local)
    };
    if matches {
        return Ok(());
    }
    let requirement = if exact {
        format!("--require-exact-version needs {LOCAL_VERSION} on `{host}`")
    } else {
        "Remote launches need the same major and minor version on both hosts".to_string()
    };
    Err(Fail::usage(
        format!("remote boxr on `{host}` is {remote_text}, local is {LOCAL_VERSION}"),
        vec![
            requirement,
            format!("Run `boxr --remote {host} --require-exact-version` only against an exactly matching build"),
            format!("Check the remote with `ssh {host} 'boxr --version'`"),
        ],
    )
    .into())
}

fn detach_command(request: &RemoteLaunch) -> String {
    let launch = shell_join(&build_detach_args(request));
    match request.dir.as_deref() {
        Some(dir) => format!("cd -- {} && exec {}", shell_quote(dir), launch),
        None => launch,
    }
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

fn run_ssh(host: &str, command: &str, context: &str) -> Result<std::process::Output> {
    let program = ssh_program()?;
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
    if let Some(dir) = &request.dir {
        toon.field("dir", dir);
    }
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
