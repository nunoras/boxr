use crate::home::create_private_dir;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Session {
    pub id: String,
    pub dir: PathBuf,
}

impl Session {
    pub fn plan(home: &Path) -> Session {
        let id = new_id();
        let dir = home.join("sessions").join(&id);
        Session { id, dir }
    }

    pub fn materialize(&self) -> Result<()> {
        create_private_dir(&self.dir)?;
        create_private_dir(&self.raw_dir())?;
        create_private_dir(&self.harness_dir())
    }

    pub fn open(home: &Path, id: &str) -> Session {
        Session {
            id: id.to_string(),
            dir: home.join("sessions").join(id),
        }
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
}

fn new_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    format!("s-{millis:x}-{:x}", std::process::id())
}
