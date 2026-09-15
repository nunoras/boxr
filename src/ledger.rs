use crate::atif::{
    Agent, Closing, FinalMetrics, Header, Observation, ObservationResult, Step, Trajectory,
    SCHEMA_VERSION,
};
use crate::harness::{Harness, TranscriptEntry};
use crate::home::restrict_file;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const SUMMARY_FILE: &str = "summary.jsonl";

const POLL: Duration = Duration::from_millis(20);
const TRANSCRIPT_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct SessionStart {
    pub harness_session_id: String,
    pub harness_version: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Seed {
    pub session_id: String,
    pub harness_id: String,
    pub model: String,
    pub effort: Option<String>,
    pub mode: String,
    pub profile: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Tally {
    pub steps: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub error: Option<String>,
}

pub struct Follower {
    handle: JoinHandle<Tally>,
    sender: Sender<SessionStart>,
    stop: Arc<AtomicBool>,
}

impl Follower {
    pub fn start(harness: Arc<dyn Harness>, path: PathBuf, seed: Seed) -> Follower {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut tally = Tally::default();
            if let Err(error) = capture(harness, &path, &seed, receiver, &flag, &mut tally) {
                tally.error.get_or_insert_with(|| format!("{error:#}"));
            }
            tally
        });
        Follower {
            handle,
            sender,
            stop,
        }
    }

    pub fn session_started(&self, start: SessionStart) {
        let _ = self.sender.send(start);
    }

    pub fn finish(self) -> Tally {
        self.stop.store(true, Ordering::SeqCst);
        drop(self.sender);
        self.handle.join().unwrap_or_else(|_| Tally {
            error: Some("the ledger follower thread panicked".to_string()),
            ..Tally::default()
        })
    }
}

fn capture(
    harness: Arc<dyn Harness>,
    path: &Path,
    seed: &Seed,
    receiver: Receiver<SessionStart>,
    stop: &AtomicBool,
    tally: &mut Tally,
) -> Result<()> {
    let mut normalizer = Normalizer {
        file: create_private_file(path)?,
        path,
        tally,
        held: Vec::new(),
        counted_responses: HashSet::new(),
    };
    let start = await_start(&receiver, stop);
    normalizer.write(&header(seed, start.as_ref()))?;

    if let Some(start) = start {
        let followed = await_transcript(harness.as_ref(), &start.harness_session_id, stop)
            .and_then(|transcript| {
                follow_file(harness.as_ref(), &transcript, &mut normalizer, stop)
            });
        if let Err(error) = followed {
            normalizer.tally.error = Some(format!("{error:#}"));
        }
    }

    normalizer.close()
}

struct Normalizer<'a> {
    file: File,
    path: &'a Path,
    tally: &'a mut Tally,
    held: Vec<Step>,
    counted_responses: HashSet<String>,
}

impl Normalizer<'_> {
    fn accept(&mut self, entry: TranscriptEntry) -> Result<()> {
        match entry {
            TranscriptEntry::Step {
                mut step,
                response_id,
            } => {
                if let Some(id) = response_id {
                    if !self.counted_responses.insert(id) {
                        step.metrics = None;
                    }
                }
                if step
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty())
                {
                    self.held.push(*step);
                    Ok(())
                } else {
                    self.append(*step)
                }
            }
            TranscriptEntry::ToolResults(results) => {
                for result in results {
                    self.fold(result);
                }
                self.append_answered()
            }
        }
    }

    fn fold(&mut self, result: ObservationResult) {
        let caller = self.held.iter_mut().find(|step| {
            step.tool_calls
                .iter()
                .flatten()
                .any(|call| call.tool_call_id == result.source_call_id)
        });
        if let Some(step) = caller {
            step.observation
                .get_or_insert_with(|| Observation {
                    results: Vec::new(),
                })
                .results
                .push(result);
        }
    }

    fn append_answered(&mut self) -> Result<()> {
        let (answered, waiting): (Vec<Step>, Vec<Step>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(is_answered);
        self.held = waiting;
        answered.into_iter().try_for_each(|step| self.append(step))
    }

    fn append(&mut self, mut step: Step) -> Result<()> {
        self.tally.steps += 1;
        step.step_id = self.tally.steps;
        if let Some(metrics) = &step.metrics {
            self.tally.prompt_tokens += metrics.prompt_tokens.unwrap_or(0);
            self.tally.completion_tokens += metrics.completion_tokens.unwrap_or(0);
            self.tally.cached_tokens += metrics.cached_tokens.unwrap_or(0);
        }
        self.write(&step)
    }

    fn write(&mut self, value: &impl Serialize) -> Result<()> {
        write_line(&mut self.file, self.path, value)
    }

    fn close(mut self) -> Result<()> {
        std::mem::take(&mut self.held)
            .into_iter()
            .try_for_each(|step| self.append(step))?;
        let closing = closing(self.tally);
        self.write(&closing)
    }
}

fn is_answered(step: &Step) -> bool {
    let results = step
        .observation
        .as_ref()
        .map(|observation| observation.results.as_slice())
        .unwrap_or_default();
    step.tool_calls.iter().flatten().all(|call| {
        results
            .iter()
            .any(|result| result.source_call_id == call.tool_call_id)
    })
}

fn await_start(receiver: &Receiver<SessionStart>, stop: &AtomicBool) -> Option<SessionStart> {
    loop {
        match receiver.recv_timeout(POLL) {
            Ok(start) => return Some(start),
            Err(RecvTimeoutError::Disconnected) => return None,
            Err(RecvTimeoutError::Timeout) if stop.load(Ordering::SeqCst) => return None,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn await_transcript(
    harness: &dyn Harness,
    harness_session_id: &str,
    stop: &AtomicBool,
) -> Result<PathBuf> {
    let mut deadline = None;
    loop {
        if let Ok(path) = harness.transcript(harness_session_id) {
            return Ok(path);
        }
        if stop.load(Ordering::SeqCst) {
            let limit = *deadline.get_or_insert_with(|| Instant::now() + TRANSCRIPT_GRACE);
            if Instant::now() >= limit {
                return Err(anyhow!(
                    "the harness never wrote a transcript for session {harness_session_id}"
                ));
            }
        }
        thread::sleep(POLL);
    }
}

fn follow_file(
    harness: &dyn Harness,
    transcript: &Path,
    normalizer: &mut Normalizer,
    stop: &AtomicBool,
) -> Result<()> {
    let mut source =
        File::open(transcript).with_context(|| format!("opening {}", transcript.display()))?;
    let mut pending = Vec::new();
    loop {
        let finished = stop.load(Ordering::SeqCst);
        source
            .read_to_end(&mut pending)
            .with_context(|| format!("reading {}", transcript.display()))?;
        while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=index).collect();
            if let Some(entry) = harness.transcript_entry(String::from_utf8_lossy(&line).trim_end())
            {
                normalizer.accept(entry)?;
            }
        }
        if finished {
            return Ok(());
        }
        thread::sleep(POLL);
    }
}

fn header(seed: &Seed, start: Option<&SessionStart>) -> Header {
    let mut extra = Map::new();
    extra.insert("harness".to_string(), Value::from(seed.harness_id.clone()));
    extra.insert("mode".to_string(), Value::from(seed.mode.clone()));
    extra.insert(
        "effort".to_string(),
        seed.effort.clone().map(Value::from).unwrap_or(Value::Null),
    );
    extra.insert(
        "profile".to_string(),
        seed.profile.clone().map(Value::from).unwrap_or(Value::Null),
    );
    extra.insert(
        "harnessSessionId".to_string(),
        start
            .map(|value| Value::from(value.harness_session_id.clone()))
            .unwrap_or(Value::Null),
    );
    Header {
        schema_version: SCHEMA_VERSION.to_string(),
        session_id: seed.session_id.clone(),
        agent: Agent {
            name: seed.harness_id.clone(),
            version: start
                .and_then(|value| value.harness_version.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            model_name: Some(
                start
                    .and_then(|value| value.model.clone())
                    .unwrap_or_else(|| seed.model.clone()),
            ),
        },
        extra,
    }
}

fn closing(tally: &Tally) -> Closing {
    let mut extra = Map::new();
    if let Some(error) = &tally.error {
        extra.insert("captureError".to_string(), Value::from(error.clone()));
    }
    Closing {
        final_metrics: FinalMetrics {
            total_prompt_tokens: tally.prompt_tokens,
            total_completion_tokens: tally.completion_tokens,
            total_cached_tokens: tally.cached_tokens,
            total_cost_usd: None,
            total_steps: tally.steps,
            extra,
        },
    }
}

fn write_line(file: &mut File, path: &Path, value: &impl Serialize) -> Result<()> {
    let line = serde_json::to_string(value).context("encoding a normalized ledger line")?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.flush())
        .with_context(|| format!("writing {}", path.display()))
}

fn create_private_file(path: &Path) -> Result<File> {
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    restrict_file(path)?;
    Ok(file)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub harness: String,
    #[serde(rename = "harnessSessionId")]
    pub harness_session_id: Option<String>,
    pub model: String,
    pub effort: Option<String>,
    pub profile: Option<String>,
    pub mode: String,
    pub start: String,
    pub end: String,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    pub status: String,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    pub steps: u64,
    #[serde(rename = "promptTokens")]
    pub prompt_tokens: u64,
    #[serde(rename = "completionTokens")]
    pub completion_tokens: u64,
    #[serde(rename = "cachedTokens")]
    pub cached_tokens: u64,
}

pub fn append_summary(path: &Path, summary: &Summary) -> Result<()> {
    let line = serde_json::to_string(summary).context("encoding the session summary")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    restrict_file(path)?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.flush())
        .with_context(|| format!("writing {}", path.display()))
}

pub fn read_summary(home: &Path, id: &str) -> Result<Summary> {
    let path = home.join(SUMMARY_FILE);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))
        .map_err(|_| anyhow!("no session {id} in the ledger"))?;
    text.lines()
        .filter_map(|line| serde_json::from_str::<Summary>(line).ok())
        .find(|summary| summary.id == id)
        .ok_or_else(|| anyhow!("no session {id} in the ledger"))
}

pub fn trajectory(normalized: &Path, summary: &Summary) -> Result<Trajectory> {
    let text = fs::read_to_string(normalized)
        .with_context(|| format!("reading {}", normalized.display()))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let head = lines
        .next()
        .ok_or_else(|| anyhow!("{} is empty", normalized.display()))?;
    let head: Value =
        serde_json::from_str(head).with_context(|| format!("reading {}", normalized.display()))?;
    let mut steps = Vec::new();
    let mut final_metrics = None;
    for line in lines {
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("reading {}", normalized.display()))?;
        match value.get("final_metrics") {
            Some(metrics) => final_metrics = Some(serde_json::from_value(metrics.clone())?),
            None => steps.push(value),
        }
    }
    let agent: Agent = serde_json::from_value(
        head.get("agent")
            .cloned()
            .ok_or_else(|| anyhow!("{} has no header line", normalized.display()))?,
    )?;
    let mut extra: Map<String, Value> = head
        .get("extra")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    extra.insert("status".to_string(), Value::from(summary.status.clone()));
    extra.insert("start".to_string(), Value::from(summary.start.clone()));
    extra.insert("end".to_string(), Value::from(summary.end.clone()));
    Ok(Trajectory {
        schema_version: SCHEMA_VERSION.to_string(),
        session_id: summary.id.clone(),
        agent,
        steps,
        final_metrics,
        extra,
    })
}

pub fn write_trajectory(path: &Path, document: &Trajectory) -> Result<()> {
    if let Some(parent) = path.parent() {
        crate::home::create_private_dir(parent)?;
    }
    let text =
        serde_json::to_string_pretty(document).context("encoding the ATIF trajectory document")?;
    fs::write(path, format!("{text}\n")).with_context(|| format!("writing {}", path.display()))?;
    restrict_file(path)
}
