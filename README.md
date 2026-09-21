# boxr

[![CI](https://github.com/nunoras/boxr/actions/workflows/ci.yml/badge.svg)](https://github.com/nunoras/boxr/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

Launch any coding agent on any harness, model, effort and account. Record every session in a local ledger. Measure what works.

boxr is a CLI that sits in front of your coding agents. It starts a headless or interactive session, tails the harness transcript live, normalizes the output into ATIF steps, and appends a summary line when the session ends. You get token counts, cost, git evidence and stats grouped however you want - without any of it leaving your machine.

## boxr records everything

Every session writes three layers: the raw harness transcript (verbatim, never modified), a normalized JSONL of ATIF steps, and one summary line. The summary carries the harness, model, effort, profile, kind, cost, token counts, exit facts, git evidence and your verdict. All of it lives in the boxr home (`~/.boxr`, or wherever `BOXR_HOME` points). There is no sync and no telemetry.

## boxr launches across harnesses

One command, any supported harness:

```
boxr --harness claude --model opus --effort high --account work --kind build "fix the login redirect"
```

Harness, model, effort and account are explicit on the command line, with fallbacks to `config.json` in the boxr home. Today boxr launches Claude Code and pi. pi models use the `provider/id` form (for example `--model xai/grok-4.5`), and `--effort` maps to pi's thinking level.

## boxr isolates accounts

Account profiles are per-harness credential directories boxr owns, under `accounts/<harness>/<name>` in the boxr home. `boxr account add --harness claude --name work` runs Claude Code's own login pointed at that isolated directory. A launch with `--account work` uses that profile. Parallel sessions on different profiles work without conflict, and boxr never reads or writes your normal harness config.

## boxr manages background sessions

`--detach` returns the session id and keeps the session running under a boxr supervisor. If boxr or the supervisor is killed, the harness process is killed with it and the session is marked interrupted.

```
boxr --harness claude --model opus --detach "add rate limiting"
boxr ps
boxr status <id>
boxr wait <id>
boxr tail <id>
boxr stop <id>
boxr resume <id> "now add the tests"
```

`boxr resume` continues a finished session with a new prompt, using the same harness, model, effort and profile.

## boxr launches over SSH

`boxr --remote <host>` probes the remote boxr version, runs a detached launch there, prints the remote session id and exits. The session is recorded in the remote ledger only.

## boxr gives you stats

`boxr stats --by model,kind --since 7d` groups the summary ledger by any combination of model, harness, effort, profile, kind, status, verdict, interrupted and limitHit. Cost comes from a price table you set in `config.json`, per model, per million tokens. boxr never fetches a price.

`boxr outcome <id> success|partial|failed --note "..."` records your verdict on a session. `boxr outcome --check-reverted <id>` checks whether the commits the session made still exist on a local branch.

## boxr exports ATIF trajectories

`boxr export --atif <id>` writes a single-document ATIF trajectory (schema ATIF-v1.8) from the normalized session data.

## boxr serves a read-only API

`boxr serve` exposes the ledger over HTTP as JSON on port 4035 (configurable with `--port`). The endpoints are `GET /ps`, `GET /status/<id>` and `GET /outcome/<id>`. Nothing that mutates is accepted.

## Get started

1. Install (requires Rust):

```
cargo install --git https://github.com/nunoras/boxr --locked
```

Or from a local clone:

```
git clone https://github.com/nunoras/boxr
cd boxr
cargo install --path . --locked
```

2. Set your defaults in `~/.boxr/config.json`:

```json
{
  "defaults": {
    "harness": "claude",
    "model": "sonnet"
  },
  "currency": "USD",
  "prices": {
    "sonnet": {
      "input": 3.0,
      "output": 15.0,
      "cached": 0.3,
      "reasoning": 15.0
    }
  }
}
```

3. Launch a session:

```
boxr "describe this codebase in one paragraph"
```

4. Check the result:

```
boxr show <id>
boxr stats --by model --since 1d
```

## Examples

Launch with a specific kind and detach:

```
boxr --harness pi --model xai/grok-4.5 --effort high --kind fix --detach "fix the flaky date parser test"
```

Set up an account profile:

```
boxr account add --harness claude --name personal
boxr --harness claude --account personal "refactor the auth module"
```

Wait for a detached session with a timeout:

```
boxr wait --timeout 300 abc123
```

Install bundled skills into your harnesses:

```
boxr skill install --harness all
```

## Limitations

- Walking skeleton. The launcher and ledger work. Evals, the prompt skill and inferred kinds are not built yet.
- Only two harnesses have launch adapters: Claude Code and pi. Codex is in the skill install table but cannot launch sessions.
- No inferred kind classification. Without `--kind`, the session is unclassified.
- Cost is an estimate from a user-maintained price table. boxr never fetches real prices.
- macOS is best-effort. Linux and Windows are the tested platforms.
- There is no import of old transcripts yet.

## References

- [depot](https://github.com/nunoras/depot) - multi-task agent coordinator that launches workers through boxr.
- [Design document](docs/design.md) - settled decisions and the full milestone plan.
- [ATIF trajectory format](https://www.harborframework.com/docs/agents/trajectory-format) - the schema boxr normalizes to.

## License

Apache-2.0. See [LICENSE](LICENSE).

## Status

boxr is a working tool in active development, not a stable release. The CLI surface may change between versions.
