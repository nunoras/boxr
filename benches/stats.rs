#[path = "../tests/common/summary.rs"]
mod summary;

use std::process::Command;
use std::time::{Duration, Instant};

use summary::{write_summary_fixture, SummaryFixture};

const SESSIONS: usize = 10_000;
const BUDGET: Duration = Duration::from_secs(3);

fn main() {
    let root = tempfile::tempdir().expect("temp dir");
    let home = root.path().join("boxr");
    for index in 0..SESSIONS {
        write_summary_fixture(
            &home,
            index,
            SummaryFixture::new("sonnet", "build", 1, 2, 3, 4, Some(0.000_002)),
        );
    }

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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("sonnet,build,USD,10000,60000,40000,0.02,0"),
        "{stdout}"
    );
    assert!(
        elapsed < BUDGET,
        "stats over {SESSIONS} sessions took {elapsed:?}, over the {BUDGET:?} budget"
    );
    println!("stats over {SESSIONS} sessions took {elapsed:?}");
}
