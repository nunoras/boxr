use crate::clock;
use crate::detached::LaunchFile;
use crate::harness::TranscriptEntry;
use crate::ledger::Snapshot;
use crate::session::Session;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct Activity {
    pub last_activity_millis: u128,
    pub current_tool: Option<String>,
}

pub fn inspect(
    home: &Path,
    session: &Session,
    launch: &LaunchFile,
    snapshot: &Snapshot,
) -> Activity {
    let transcript = transcript_path(
        home,
        session,
        launch,
        snapshot.harness_session_id.as_deref(),
    );
    let current_tool = pending_in_ledger(&snapshot.steps).or_else(|| {
        transcript
            .as_deref()
            .and_then(|path| pending_in_transcript(launch, path))
    });
    summarise(
        &snapshot.steps,
        session,
        launch,
        transcript.as_deref(),
        current_tool,
    )
}

pub fn inspect_ledger(session: &Session, launch: &LaunchFile, snapshot: &Snapshot) -> Activity {
    let current_tool = pending_in_ledger(&snapshot.steps);
    summarise(&snapshot.steps, session, launch, None, current_tool)
}

fn summarise(
    steps: &[Value],
    session: &Session,
    launch: &LaunchFile,
    transcript: Option<&Path>,
    current_tool: Option<String>,
) -> Activity {
    let last_activity_millis = latest_timestamp(steps)
        .into_iter()
        .chain(transcript.and_then(file_mtime))
        .max()
        .or_else(|| file_mtime(&session.normalized_path()))
        .unwrap_or(launch.started_millis as u128);
    Activity {
        last_activity_millis,
        current_tool,
    }
}

fn latest_timestamp(steps: &[Value]) -> Option<u128> {
    steps
        .iter()
        .filter_map(|step| step.get("timestamp").and_then(Value::as_str))
        .filter_map(clock::parse_iso8601)
        .max()
}

fn pending_in_ledger(steps: &[Value]) -> Option<String> {
    let answered: HashSet<String> = steps
        .iter()
        .flat_map(|step| {
            step.get("observation")
                .and_then(|observation| observation.get("results"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|result| result.get("source_call_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let mut pending = None;
    for step in steps {
        for call in step
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = call
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if answered.contains(id) {
                continue;
            }
            pending = Some(
                call.get("function_name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
            );
        }
    }
    pending
}

fn transcript_path(
    home: &Path,
    session: &Session,
    launch: &LaunchFile,
    harness_session_id: Option<&str>,
) -> Option<PathBuf> {
    let adapter = crate::harness::lookup(&launch.harness)?;
    let account = launch
        .profile
        .as_deref()
        .and_then(|name| crate::account::resolve(home, &launch.harness, name).ok());
    let harness_session = crate::run::harness_session_of(session);
    adapter
        .transcript(&harness_session, harness_session_id?, account.as_deref())
        .ok()
}

fn pending_in_transcript(launch: &LaunchFile, path: &Path) -> Option<String> {
    let adapter = crate::harness::lookup(&launch.harness)?;
    let text = fs::read_to_string(path).ok()?;
    let text = text.get(launch.from_bytes as usize..).unwrap_or_default();
    let mut calls: Vec<(String, String)> = Vec::new();
    let mut answered: HashSet<String> = HashSet::new();
    for line in text.lines() {
        match adapter.transcript_entry(line) {
            Some(TranscriptEntry::Step { step, .. }) => {
                for call in step.tool_calls.iter().flatten() {
                    calls.push((call.tool_call_id.clone(), call.function_name.clone()));
                }
            }
            Some(TranscriptEntry::ToolResults(results)) => {
                for result in results {
                    answered.insert(result.source_call_id);
                }
            }
            None => {}
        }
    }
    calls
        .into_iter()
        .rev()
        .find(|(id, _)| !answered.contains(id))
        .map(|(_, name)| name)
}

fn file_mtime(path: &Path) -> Option<u128> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|age| age.as_millis())
}
