# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Rust single binary crate. Gates: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. CI runs all three on Linux and Windows (`.github/workflows/ci.yml`).
- The design is the settled contract: `docs/design.md`. Tickets live on GitHub under nunoras/boxr.
- Tests use exactly one seam: the `boxr` binary as a black box against a throwaway boxr home (`BOXR_HOME`), with fake harness executables first on `PATH`. See `tests/headless.rs` and `examples/fake-claude.rs`; fixtures are redacted recordings of real harness output under `tests/fixtures/`. Do not add unit tests behind that seam.
- New harnesses implement the `Harness` trait in `src/harness/mod.rs`; the process runner in `src/run.rs` stays harness-agnostic.
  Transcript following lives in `src/ledger.rs` and is harness-agnostic too: a harness only maps one transcript line to one ATIF step or a batch of tool results through `transcript_entry`, and the follower folds results into the calling step (see "Tool calls in the normalized layer" in `docs/design.md`).
- The normalized ledger targets ATIF v1.8 (`src/atif.rs`); the schema is the Harbor RFC at https://www.harborframework.com/docs/agents/trajectory-format.
- Output is axi-style TOON on stdout with `help[]` next-step lines; exit codes are defined in `src/fail.rs`.
- No comments in code, per the repo's coding standard.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
