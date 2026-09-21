use std::fs;
use std::process::Command;
use std::time::Instant;

const SESSIONS: usize = 10_000;

fn main() {
    let root = tempfile::tempdir().expect("temp dir");
    let home = root.path().join("boxr");
    fs::create_dir_all(&home).expect("boxr home");
    let mut ledger = String::new();
    for index in 0..SESSIONS {
        ledger.push_str(&summary_line(index));
    }
    fs::write(home.join("summary.jsonl"), ledger).expect("ledger");

    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_boxr"))
        .args(["stats", "--by", "model,kind", "--since", "7d"])
        .env("BOXR_HOME", &home)
        .output()
        .expect("boxr runs");
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("stats over {SESSIONS} sessions took {elapsed:?}");
}

fn summary_line(index: usize) -> String {
    format!(
        "{{\"id\":\"s-{index}\",\"harness\":\"claude\",\"model\":\"sonnet\",\"effort\":\"high\",\"profile\":\"work\",\"mode\":\"headless\",\"start\":\"2099-01-01T00:00:00.000Z\",\"end\":\"2099-01-01T00:00:00.004Z\",\"durationMs\":4,\"status\":\"ok\",\"exitCode\":0,\"steps\":1,\"promptTokens\":1,\"completionTokens\":2,\"cachedTokens\":3,\"reasoningTokens\":0,\"apiEquivalentCost\":0.000002,\"currency\":\"USD\",\"kind\":\"build\",\"kindSource\":\"declared\"}}\n"
    )
}
