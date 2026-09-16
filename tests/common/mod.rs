use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

pub fn example_binary(name: &str) -> PathBuf {
    let boxr = Path::new(env!("CARGO_BIN_EXE_boxr"));
    let path = boxr
        .parent()
        .expect("target directory")
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "the {name} example is missing at {}; build it with `cargo build --example {name}`",
        path.display()
    );
    path
}

pub fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub fn session_id_of(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("id: "))
        .expect("session id line")
        .to_string()
}

pub fn normalized_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("normalized ledger")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("a json line"))
        .collect()
}

pub fn summary_of(home: &Path, id: &str) -> Value {
    fs::read_to_string(home.join("summary.jsonl"))
        .expect("summary ledger")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("a json line"))
        .find(|value| value["id"] == id)
        .expect("a summary line for the session")
}

pub fn sources_of(steps: &[Value]) -> Vec<&str> {
    steps
        .iter()
        .map(|step| step["source"].as_str().expect("step source"))
        .collect()
}

pub fn assert_valid_steps(steps: &[Value]) {
    for (index, step) in steps.iter().enumerate() {
        let none = Vec::new();
        assert_eq!(step["step_id"], (index + 1) as i64, "{step}");
        let source = step["source"].as_str().expect("step source");
        assert!(
            matches!(source, "system" | "user" | "agent"),
            "unknown source {source}"
        );
        assert!(step["message"].is_string(), "{step}");
        if let Some(timestamp) = step["timestamp"].as_str() {
            assert!(
                timestamp.ends_with('Z') && timestamp.contains('T'),
                "{step}"
            );
        }
        if source != "agent" {
            for field in [
                "model_name",
                "reasoning_effort",
                "reasoning_content",
                "tool_calls",
                "observation",
                "metrics",
            ] {
                assert!(step.get(field).is_none(), "{field} on a {source} step");
            }
        }
        let mut call_ids = Vec::new();
        for call in step["tool_calls"].as_array().unwrap_or(&none) {
            call_ids.push(call["tool_call_id"].as_str().expect("tool_call_id"));
            assert!(call["function_name"].is_string(), "{call}");
            assert!(call["arguments"].is_object(), "{call}");
        }
        for result in step["observation"]["results"].as_array().unwrap_or(&none) {
            assert!(result["content"].is_string(), "{result}");
            if let Some(source_call_id) = result["source_call_id"].as_str() {
                assert!(
                    call_ids.contains(&source_call_id),
                    "observation {source_call_id} has no tool call on step {step}"
                );
            }
        }
    }
}

pub fn assert_valid_atif(document: &Value) {
    assert_eq!(document["schema_version"], "ATIF-v1.8");
    assert!(document["session_id"].is_string(), "{document}");
    assert!(document["agent"]["name"].is_string(), "{document}");
    assert!(document["agent"]["version"].is_string(), "{document}");
    let steps = document["steps"].as_array().expect("steps array");
    assert!(!steps.is_empty(), "{document}");
    assert_valid_steps(steps);
    let metrics = &document["final_metrics"];
    assert_eq!(metrics["total_steps"], steps.len() as i64, "{document}");
    assert!(metrics["total_prompt_tokens"].is_u64(), "{document}");
}
