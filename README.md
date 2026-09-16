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
While a session runs, boxr follows the harness transcript and writes the normalized ledger live: `normalized.jsonl` is a header line, one ATIF step per line, and a closing line of final metrics.
Each finished session appends one line to `summary.jsonl` in the boxr home.
`boxr show <id>` prints that summary, and `boxr export --atif <id>` writes a single-document ATIF trajectory (schema ATIF-v1.8).
`boxr skill install --harness claude|codex|pi|all` installs the bundled skills into the selected harnesses, and the prompt skill ships as a placeholder for now.
Harness, model, effort and account fall back to `defaults.harness`, `defaults.model`, `defaults.effort` and `defaults.account` in `config.json` in the boxr home.
Account profiles are isolated per-harness config directories boxr owns, under `accounts/<harness>/<name>` in the boxr home.
`boxr account add --harness claude --name work` runs Claude Code's own login pointed at that directory, `boxr account list` prints the profiles as TOON, and `boxr account remove --harness claude --name work --yes` deletes one.
`--account work` on a launch points the harness at that profile; boxr never reads or writes the user's normal harness setup.
Everything else in the design is still ahead.
The design lives in [docs/design.md](docs/design.md).

## Build

```
cargo build --release
cargo test
```

## License

Apache-2.0.
