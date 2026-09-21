use crate::home::create_private_dir;
use crate::ledger::SUMMARY_FILE;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Session {
    pub id: String,
    pub home: PathBuf,
    pub dir: PathBuf,
}

impl Session {
    pub fn plan(home: &Path) -> Session {
        let id = new_id();
        let dir = home.join("sessions").join(&id);
        Session {
            id,
            home: home.to_path_buf(),
            dir,
        }
    }

    pub fn materialize(&self) -> Result<()> {
        create_private_dir(&self.dir)?;
        create_private_dir(&self.raw_dir())?;
        create_private_dir(&self.harness_dir())
    }

    pub fn open(home: &Path, id: &str) -> Session {
        Session {
            id: id.to_string(),
            home: home.to_path_buf(),
            dir: home.join("sessions").join(id),
        }
    }

    pub fn ids(home: &Path) -> Result<Vec<String>> {
        let sessions = home.join("sessions");
        let entries = match fs::read_dir(&sessions) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(
                    anyhow::Error::from(error).context(format!("reading {}", sessions.display()))
                )
            }
        };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        ids.sort();
        Ok(ids)
    }

    pub fn raw_dir(&self) -> PathBuf {
        self.dir.join("raw")
    }

    pub fn normalized_path(&self) -> PathBuf {
        self.dir.join("normalized.jsonl")
    }

    pub fn trajectory_path(&self) -> PathBuf {
        self.dir.join("export").join("trajectory.atif.json")
    }

    pub fn stream_path(&self) -> PathBuf {
        self.raw_dir().join("stream.jsonl")
    }

    pub fn stderr_path(&self) -> PathBuf {
        self.raw_dir().join("stderr.log")
    }

    pub fn transcript_path(&self) -> PathBuf {
        self.raw_dir().join("transcript.jsonl")
    }

    pub fn harness_dir(&self) -> PathBuf {
        self.dir.join("harness")
    }

    pub fn launch_path(&self) -> PathBuf {
        self.dir.join("launch.json")
    }

    pub fn supervisor_path(&self) -> PathBuf {
        self.dir.join("supervisor.json")
    }

    pub fn supervisor_log_path(&self) -> PathBuf {
        self.dir.join("supervisor.log")
    }

    pub fn report_path(&self) -> PathBuf {
        self.dir.join("report.json")
    }

    pub fn stop_path(&self) -> PathBuf {
        self.dir.join("stop")
    }

    pub fn summary_path(&self) -> PathBuf {
        self.home.join(SUMMARY_FILE)
    }
}

fn new_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    format!("s-{millis:x}-{:x}", std::process::id())
}
