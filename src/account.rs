use crate::fail::{Fail, EXIT_SESSION_FAILED};
use crate::harness::{apply_config_dir, Harness};
use crate::home::create_private_dir;
use crate::run;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const ACCOUNTS_DIR: &str = "accounts";

pub struct Profile {
    pub harness: String,
    pub name: String,
    pub dir: PathBuf,
}

pub fn profile_dir(home: &Path, harness_id: &str, name: &str) -> PathBuf {
    home.join(ACCOUNTS_DIR).join(harness_id).join(name)
}

pub fn validate_name(name: &str) -> Result<()> {
    let safe = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if safe {
        return Ok(());
    }
    Err(Fail::usage(
        format!("invalid account name `{name}`"),
        vec!["Use letters, digits, dashes, underscores and dots, for example `work`".to_string()],
    )
    .into())
}

pub fn add(harness: &dyn Harness, home: &Path, name: &str) -> Result<(PathBuf, bool)> {
    validate_name(name)?;
    let dir = profile_dir(home, harness.id(), name);
    let existed = dir.is_dir();
    create_private_dir(&dir)?;

    let login = login(harness, &dir);
    if !existed && !matches!(login, Ok(0)) {
        fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    match login? {
        0 => Ok((dir, existed)),
        code => Err(login_failure(harness.id(), name, code, existed)),
    }
}

pub fn resolve(home: &Path, harness_id: &str, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let dir = profile_dir(home, harness_id, name);
    if dir.is_dir() {
        return Ok(dir);
    }
    Err(Fail::usage(
        format!("unknown account `{name}` for harness `{harness_id}`"),
        vec![
            "Run `boxr account list` to see the profiles that exist".to_string(),
            format!("Run `boxr account add --harness {harness_id} --name {name}` to create it"),
        ],
    )
    .into())
}

pub fn list(home: &Path) -> Result<Vec<Profile>> {
    let root = home.join(ACCOUNTS_DIR);
    let harnesses = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(anyhow::Error::from(error).context(format!("reading {}", root.display())))
        }
    };

    let mut profiles = Vec::new();
    for harness in harnesses.flatten() {
        let harness_id = harness.file_name().to_string_lossy().to_string();
        let Ok(names) = fs::read_dir(harness.path()) else {
            continue;
        };
        for name in names.flatten() {
            let dir = name.path();
            if !dir.is_dir() {
                continue;
            }
            profiles.push(Profile {
                harness: harness_id.clone(),
                name: name.file_name().to_string_lossy().to_string(),
                dir,
            });
        }
    }
    profiles.sort_by(|left, right| (&left.harness, &left.name).cmp(&(&right.harness, &right.name)));
    Ok(profiles)
}

pub fn remove(home: &Path, harness_id: &str, name: &str) -> Result<PathBuf> {
    let dir = resolve(home, harness_id, name)?;
    fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    Ok(dir)
}

fn login(harness: &dyn Harness, dir: &Path) -> Result<i32> {
    let mut command = harness.login_command()?;
    apply_config_dir(&mut command, harness, Some(dir));
    let program = run::locate(&command.program).ok_or_else(|| {
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
    })?;

    let status = Command::new(&program)
        .args(&command.args)
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("starting {} login", harness.id()))?;

    Ok(run::exit_code_of(&status))
}

fn login_failure(harness_id: &str, name: &str, code: i32, kept_profile: bool) -> anyhow::Error {
    let mut help = vec![format!(
        "Run `boxr account add --harness {harness_id} --name {name}` to try again"
    )];
    if kept_profile {
        help.push(format!(
            "Run `boxr account remove --harness {harness_id} --name {name} --yes` to delete the profile"
        ));
    }
    Fail {
        code: EXIT_SESSION_FAILED,
        message: format!("{harness_id} login for account `{name}` failed with exit code {code}"),
        help,
    }
    .into()
}
