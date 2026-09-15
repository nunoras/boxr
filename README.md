# boxr

An agent-first CLI to launch any coding agent on any harness, model, effort and account, record every session, and measure what works.

```
boxr --harness claude --model opus --effort high --account work --kind build "fix the login redirect"
```

boxr does two jobs.
It launches agents across harnesses and isolated account profiles.
It keeps a local ledger of every session, so you can see which models you use for what, where tokens go, and test prompt changes against real scenarios instead of guessing.

Status: design.
Nothing is built yet.
The design lives in [docs/design.md](docs/design.md).

## License

Apache-2.0.
