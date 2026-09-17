use crate::atif::{
    Agent, Closing, FinalMetrics, Header, Observation, ObservationResult, Step, Trajectory,
    SCHEMA_VERSION,
};
use crate::harness::{Harness, HarnessSession, TranscriptEntry};
use crate::home::restrict_file;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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
    pub resumed_from: Option<String>,
    pub kind: Option<String>,
    pub kind_source: Option<String>,
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
    pub fn start(
        harness: Arc<dyn Harness>,
        session: HarnessSession,
        path: PathBuf,
        seed: Seed,
        account: Option<PathBuf>,
        from_bytes: u64,
    ) -> Follower {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut tally = Tally::default();
            let source = Source {
                harness,
                session,
                account,
                from_bytes,
            };
            if let Err(error) = capture(&source, &path, &seed, receiver, &flag, &mut tally) {
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

struct Source {
    harness: Arc<dyn Harness>,
    session: HarnessSession,
    account: Option<PathBuf>,
    from_bytes: u64,
}

fn capture(
    source: &Source,
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
        let harness = source.harness.as_ref();
        let followed =
            await_transcript(source, &start.harness_session_id, stop).and_then(|transcript| {
                follow_file(
                    harness,
                    &transcript,
                    source.from_bytes,
                    &mut normalizer,
                    stop,
                )
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
    source: &Source,
    harness_session_id: &str,
    stop: &AtomicBool,
) -> Result<PathBuf> {
    let mut deadline = None;
    loop {
        if let Ok(path) = source.harness.transcript(
            &source.session,
            harness_session_id,
            source.account.as_deref(),
        ) {
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
    from_bytes: u64,
    normalizer: &mut Normalizer,
    stop: &AtomicBool,
) -> Result<()> {
    let mut source =
        File::open(transcript).with_context(|| format!("opening {}", transcript.display()))?;
    source
        .seek(SeekFrom::Start(from_bytes))
        .with_context(|| format!("seeking in {}", transcript.display()))?;
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
        "resumedFrom".to_string(),
        seed.resumed_from
            .clone()
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    extra.insert(
        "kind".to_string(),
        seed.kind.clone().map(Value::from).unwrap_or(Value::Null),
    );
    extra.insert(
        "kindSource".to_string(),
        seed.kind_source
            .clone()
            .map(Value::from)
            .unwrap_or(Value::Null),
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
    #[serde(default, rename = "harnessSessionId")]
    pub harness_session_id: Option<String>,
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    pub mode: String,
    #[serde(default, rename = "resumedFrom")]
    pub resumed_from: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, rename = "kindSource")]
    pub kind_source: Option<String>,
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
    #[serde(default)]
    pub interrupted: bool,
    #[serde(default, rename = "limitHit")]
    pub limit_hit: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default, rename = "verdictNote")]
    pub verdict_note: Option<String>,
    #[serde(default)]
    pub git: Option<crate::git::Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryUpdate {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "verdictNote",
        deserialize_with = "double_option"
    )]
    pub verdict_note: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<crate::git::Evidence>,
}

fn double_option<'de, D>(deserializer: D) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

impl SummaryUpdate {
    fn apply(self, summary: &mut Summary) {
        if let Some(verdict) = self.verdict {
            summary.verdict = Some(verdict);
        }
        if let Some(verdict_note) = self.verdict_note {
            summary.verdict_note = verdict_note;
        }
        if let Some(git) = self.git {
            summary.git = Some(git);
        }
    }
}

pub fn append_summary(path: &Path, summary: &Summary) -> Result<()> {
    append_summary_record(path, summary)
}

pub fn append_summary_update(path: &Path, update: &SummaryUpdate) -> Result<()> {
    append_summary_record(path, update)
}

fn append_summary_record(path: &Path, record: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_string(record).context("encoding the session summary")?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    restrict_file(path)?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.flush())
        .with_context(|| format!("writing {}", path.display()))
}

pub fn read_summary(home: &Path, id: &str) -> Result<Summary> {
    find_summary(home, id)?.ok_or_else(|| anyhow!("no session {id} in the ledger"))
}

pub fn find_summary(home: &Path, id: &str) -> Result<Option<Summary>> {
    let path = home.join(SUMMARY_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::Error::from(error).context(format!("reading {}", path.display())))
        }
    };
    let mut summary = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        if value.get("status").is_some() {
            if let Ok(record) = serde_json::from_value::<Summary>(value) {
                summary = Some(record);
            }
        } else if let (Some(summary), Ok(update)) = (
            summary.as_mut(),
            serde_json::from_value::<SummaryUpdate>(value),
        ) {
            update.apply(summary);
        }
    }
    Ok(summary)
}

pub fn totals(normalized: &Path) -> Tally {
    let mut tally = Tally::default();
    let Ok(text) = fs::read_to_string(normalized) else {
        return tally;
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(metrics) = value.get("final_metrics") {
            return serde_json::from_value::<FinalMetrics>(metrics.clone())
                .map(|metrics| Tally {
                    steps: metrics.total_steps,
                    prompt_tokens: metrics.total_prompt_tokens,
                    completion_tokens: metrics.total_completion_tokens,
                    cached_tokens: metrics.total_cached_tokens,
                    error: None,
                })
                .unwrap_or(tally);
        }
        if value.get("step_id").is_none() {
            continue;
        }
        tally.steps += 1;
        if let Some(metrics) = value.get("metrics") {
            tally.prompt_tokens += metrics
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            tally.completion_tokens += metrics
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            tally.cached_tokens += metrics
                .get("cached_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
        }
    }
    tally
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
