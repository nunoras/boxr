# boxr

An agent-first CLI to launch any coding agent on any harness, model, effort and account, record every session, and measure what works.

```
boxr --harness claude --model opus --effort high --account work --kind build "fix the login redirect"
```

boxr does two jobs.
It launches agents across harnesses and isolated account profiles.
It keeps a local ledger of every session, so you can see which models you use for what, where tokens go, and test prompt changes against real scenarios instead of guessing.

Status: walking skeleton.
Headless Claude Code launches run and land in the raw ledger under the boxr home (`~/.boxr`, or `BOXR_HOME` when set).
Harness, model and effort fall back to `defaults.harness`, `defaults.model` and `defaults.effort` in `config.json` in the boxr home.
`boxr skill install --harness claude|codex|pi|all` installs the bundled skills into the selected harnesses, and the prompt skill ships as a placeholder for now.
Everything else in the design is still ahead.
The design lives in [docs/design.md](docs/design.md).

## Build

```
cargo build --release
cargo test
```

## License

Apache-2.0.
