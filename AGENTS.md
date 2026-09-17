# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Rust single binary crate. Gates: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. CI runs all three on Linux and Windows (`.github/workflows/ci.yml`).
- The design is the settled contract: `docs/design.md`. Tickets live on GitHub under nunoras/boxr.
- Tests use exactly one seam: the `boxr` binary as a black box against a throwaway boxr home (`BOXR_HOME`), with fake harness executables first on `PATH`. Each harness has its own test file and fake under `tests/` and `examples/` (`tests/headless.rs` with `examples/fake-claude.rs`, `tests/pi.rs` with `examples/fake-pi.rs`), detached sessions have `tests/detached.rs`, resumed sessions `tests/resume.rs`, the outcome signals and git evidence have `tests/outcomes.rs`, and they share the ATIF assertions in `tests/common/mod.rs`; fixtures are redacted recordings of real harness output under `tests/fixtures/<harness>/`. Do not add unit tests behind that seam. Failure injection at that seam uses `BOXR_TEST_FAIL_JOB_GUARD` and `BOXR_TEST_FAIL_SUPERVISOR_RECORD`.
- Proving boxr against real harnesses is the `verify-boxr` skill in `.claude/skills/verify-boxr/`: `scripts/verify-boxr.sh <feature>` builds the release binary, drives a real harness against a throwaway `BOXR_HOME`, keeps evidence under `~/.boxr-verify` and never runs in CI. Each new user-facing feature adds a file to its `features/` map.
- New harnesses implement the `Harness` trait in `src/harness/mod.rs`; the process runner in `src/run.rs` stays harness-agnostic.
  A harness that must be told where to write its transcript gets `HarnessSession` in `command` and `transcript`, and shared JSON block helpers live in `src/harness/json.rs`.
  Transcript following lives in `src/ledger.rs` and is harness-agnostic too: a harness only maps one transcript line to one ATIF step or a batch of tool results through `transcript_entry`, and the follower folds results into the calling step (see "Tool calls in the normalized layer" in `docs/design.md`).
  A harness adapter also provides its own `login_command` and its `config_dir_env` override, which is what account profiles isolate through; a harness without one cannot be given a profile.
  Resuming is the same seam plus `LaunchMode` in the `LaunchRequest`; a continuation is a new session linked by `resumedFrom` whose follower starts at the byte offset the original harness transcript had already reached, so a harness that supports resume must keep appending to its original transcript (see "Resuming a finished session" in `docs/design.md`).
- Every headless launch, blocking or detached, records itself in its session directory (`launch.json`, `supervisor.json`, `report.json`) and is observed through `src/detached.rs`; `--detach` spawns `boxr __supervise <id>` to keep the session running.
- `summary.jsonl` is append-only: a session's full summary is one record, and `boxr outcome` and `boxr outcome --check-reverted` append field-scoped records carrying only the fields they change, so every reader (`ledger::find_summary`, `stats`) folds a session's records in order instead of taking the last raw line; never reintroduce a whole-file rewrite or a whole-snapshot update, which drop a signal written concurrently.
- A session's outcome signals stay separate fields rather than one rolled-up status; git evidence (`src/git.rs`) is the commits reachable from the exit `HEAD` that the launch `HEAD` did not reach, with its over-counting limits documented in "Outcomes" in `docs/design.md`, and `--check-reverted` marks a commit reverted only when no local branch contains it.
- The normalized ledger targets ATIF v1.8 (`src/atif.rs`); the schema is the Harbor RFC at https://www.harborframework.com/docs/agents/trajectory-format.
- Account profiles live at `accounts/<harness>/<name>` under the boxr home (`src/account.rs`). boxr only ever sets the config-dir override and never touches the user's own harness config.
- Output is axi-style TOON on stdout with `help[]` next-step lines; exit codes are defined in `src/fail.rs`.
- No comments in code, per the repo's coding standard.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
