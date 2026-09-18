# boxr

An agent-first CLI to launch any coding agent on any harness, model, effort and account, record every session, and measure what works.

```
boxr --harness claude --model opus --effort high --account work --kind build "fix the login redirect"
```

boxr does two jobs.
It launches agents across harnesses and isolated account profiles.
It keeps a local ledger of every session, so you can see which models you use for what, where tokens go, and test prompt changes against real scenarios instead of guessing.

Status: walking skeleton.
Headless Claude Code and pi launches run and land in the raw ledger under the boxr home (`~/.boxr`, or `BOXR_HOME` when set).
pi models are given in pi's `provider/id` form, for example `--harness pi --model xai/grok-4.5`, and `--effort` becomes pi's thinking level.
A launch records itself under the boxr home as it starts, so `boxr ps` lists running sessions, `boxr status <id>` reports one without blocking, `boxr wait <id>` blocks until it ends and prints the result a blocking launch would have printed, `boxr tail <id>` streams the normalized ledger as it is appended, and `boxr stop <id>` ends a session.
`--detach` returns the session id immediately and keeps the session running under a boxr supervisor process; killing that supervisor kills the harness with it and records the session as interrupted.
Those commands print a shape other tools read: `boxr ps` prints `sessions[N]{id,state,harness,model}:` with one row per running session, `boxr status <id>` prints `state:` as one of `running`, `finished`, `stopped`, `interrupted` or `failed`, and `boxr wait <id>` prints the turn outcome as `status:` as `ok`, `failed`, `interrupted` or `running`.
`boxr wait` exits zero whenever it can report one of those outcomes, including an expired `--timeout`, and `boxr resume` exits zero once it has recorded the continuation, even when the harness turn failed or the ledger could not be written.
Both only exit non-zero when boxr cannot run at all: an unknown session or records it cannot read.
`boxr resume <id> "<prompt>"` continues a finished session with a new prompt, using the same harness, model, effort and profile, and records the continuation as its own session linked to the original.
While a session runs, boxr follows the harness transcript and writes the normalized ledger live: `normalized.jsonl` is a header line, one ATIF step per line, and a closing line of final metrics.
Each finished session appends one line to `summary.jsonl` in the boxr home.
That line carries the exit facts (exit code and interruption always, plus the harness error and limit hit when the harness reports them, which the claude and pi adapters do) and the git evidence for the working directory: the commits the session made and the files it changed.
`boxr outcome <id> success|partial|failed --note "..."` records your own verdict on a session, and `boxr outcome --check-reverted <id>` re-checks the recorded commits and marks the ones no local branch contains as reverted, leaving the rest unknown.
`--kind <kind>` records a declared core kind (`build`, `fix`, `research`, `plan`, `review`, `chore` or `docs`) or a custom kind from `kinds` in `config.json`.
`boxr stats --by model,kind --since 7d` groups the summary ledger by model, harness, effort, profile, kind, status, verdict, interrupted or limitHit.
`boxr show <id>` prints that summary, and `boxr export --atif <id>` writes a single-document ATIF trajectory (schema ATIF-v1.8).
`boxr skill install --harness claude|codex|pi|all` installs the bundled skills into the selected harnesses, and the prompt skill ships as a placeholder for now.
Harness, model, effort and account fall back to `defaults.harness`, `defaults.model`, `defaults.effort` and `defaults.account` in `config.json` in the boxr home.
`config.json` also carries the money measure: `currency` is `USD` or `EUR`, and `prices` gives each model its cost per million tokens for `input`, `output`, `cached` and `reasoning`, for example `{"currency":"USD","prices":{"your-model":{"input":1.0,"output":2.0,"cached":0.1,"reasoning":2.0}}}`.
Every summary line records the session's API-equivalent cost in that currency, or `unknown` when the model has no price, and `boxr stats` sums and groups it.
The price table is the whole source of prices: boxr never fetches one.
Account profiles are isolated per-harness config directories boxr owns, under `accounts/<harness>/<name>` in the boxr home.
`boxr account add --harness claude --name work` runs Claude Code's own login pointed at that directory, `boxr account list` prints the profiles as TOON, and `boxr account remove --harness claude --name work --yes` deletes one.
`--account work` on a launch points the harness at that profile; boxr never reads or writes the user's normal harness setup.
Everything else in the design is still ahead.
The design lives in [docs/design.md](docs/design.md).

## Install

```
cargo install --path .
```

That puts `boxr` in `~/.cargo/bin`, which is on `PATH` for a normal Rust install, and `boxr --version` then reports the installed version.
To install from the repository instead of a local checkout:

```
cargo install --git https://github.com/nunoras/boxr
```

Once the release is tagged, that becomes:

```
cargo install --git https://github.com/nunoras/boxr --tag v0.2.0
```

Tagging `v0.2.0` is the captain's release step.

## Build

```
cargo build --release
cargo test
```

## License

Apache-2.0.
