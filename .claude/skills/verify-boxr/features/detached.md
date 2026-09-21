# detached sessions

`--detach` returns a session id straight away and leaves the session running under a `boxr __supervise <id>` process in its own process session.
`boxr ps`, `status`, `tail`, `wait` and `stop` observe and end it.
The shapes of `ps`, `status` and `wait` are a fixed contract depot reads (`tests/contract.rs`).

## Sub-features

- `--detach`: prints `session:` with `id`, `status: running`, `harness`, `model`, `effort` and the supervisor `pid`, and exits 0.
- `boxr ps`: `ps:` with `running: N`, then `sessions[N]{id,state,harness,model}:` with one row per live session.
- `boxr status <id>`: a running session prints `state: running`, `started`, `steps` and `pid` without blocking; a finished one prints the launch result.
- `boxr tail <id>`: streams `normalized.jsonl` as it is appended and exits 0 once the session is over.
- `boxr wait [--timeout S] <id>`: blocks until the result exists and prints it; exits 0 for every outcome.
- `boxr stop <id>`: writes a stop file the supervisor watches; on a finished session it reports `action: already-finished` and changes nothing.

## Reach it

```
boxr --harness claude --model haiku --detach "reply with the single word ok"
boxr ps
boxr status <id>
boxr tail <id>
boxr wait <id>
boxr stop <id>
```

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh detached
```

One paid session.
From the scratch directory next to the throwaway home the driver runs:

```
BOXR_HOME=<throwaway>/home boxr --harness claude --model haiku --effort low --kind describe --detach \
  -- "Reply with the single word ok and nothing else."
boxr ps
boxr status <id>
boxr tail <id>
boxr wait --timeout 300 <id>
boxr status <id>
boxr ps
boxr stop <id>
boxr show <id>
boxr export --atif <id>
```

`ps` and the first `status` run immediately after the detach returns, while Claude Code is still starting, so they see the session live.
`tail` then blocks until the session ends, which is what makes the rest of the drive see a finished session.
After `wait` the driver gives the supervisor 10 seconds to exit on its own and fails if it is still alive.

## End state

| evidence file | must hold |
|---|---|
| `detach.txt` | `status: running` and a numeric `pid` |
| `ps.txt` | the row `<id>,running,claude,haiku` |
| `status-running.txt` | `state: running` and the same `pid` |
| `tail.txt` | byte-identical to the session's `normalized.jsonl` |
| `wait.txt` | `status: ok`, `state: finished`, `ledger: recorded`, a `harnessSessionId` |
| `status-finished.txt` | `state: finished` |
| `ps-after.txt` | `running: 0` |
| `stop.txt` | `action: already-finished` and `status: ok` |
| `show.txt` | `status: ok`, `mode: headless`, `kind: describe`, `kindSource: declared` |
| `export.txt`, `trajectory.atif.json` | see [ledger-reads.md](ledger-reads.md) |

`sessions/<id>/` holds `launch.json`, `supervisor.json`, `report.json`, `supervisor.log`, `normalized.jsonl` and `raw/` with a non-empty `stream.jsonl` and `transcript.jsonl`.
`meta.txt` records `supervisorPid`, `supervisorExited: yes` and the exit code of every command.

## Gotchas

- If `ps` does not list the session the driver fails instead of skipping, because the running path would then be unproven.
  A Claude cold start takes seconds, so this means something is wrong with the supervisor record, not that haiku was fast.
- The supervisor is not in the driver's process group.
  Cleanup kills it by the pid `--detach` printed, only if it is still alive.
- `stop` is only driven against a finished session, since stopping the live one would turn the proof into an `interrupted` result.
  Stopping a live session leaves `status: interrupted`, `state: interrupted` and exit code 137 in the summary; that path is proven only by `tests/detached.rs` and the pi dogfood.
- `status` never prints `state: stopped`; a session ended from outside reads `interrupted`.
- `wait --timeout` that expires prints the running view with `status: running` and still exits 0.
