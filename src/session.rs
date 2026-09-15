use crate::home::create_private_dir;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Session {
    pub id: String,
    pub dir: PathBuf,
}

impl Session {
    pub fn create(home: &Path) -> Result<Session> {
        let id = new_id();
        let dir = home.join("sessions").join(&id);
        create_private_dir(&dir)?;
        create_private_dir(&dir.join("raw"))?;
        Ok(Session { id, dir })
    }

    pub fn raw_dir(&self) -> PathBuf {
        self.dir.join("raw")
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
}

fn new_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    format!("s-{millis:x}-{:x}", std::process::id())
}
