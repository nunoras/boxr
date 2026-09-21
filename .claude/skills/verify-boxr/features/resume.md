# resume

`boxr resume <id> "<prompt>"` continues a finished session with a new prompt, on the harness, model, effort, profile and harness session id the original recorded.
The continuation is a new session with its own id and `mode: resume`, linked back by `resumedFrom`.
Claude Code is driven with `--resume <harness session id>`, so it appends to the original transcript, and the continuation's ledger starts at the byte offset that transcript had already reached.

## Sub-features

- Blocking continuation that prints the launch result shape, with `resumedFrom`.
- The original session's records are never rewritten.
- Refusals: an unknown id, a session still running, one with no harness session id, or one whose harness transcript is gone are usage errors (exit 2).

## Reach it

```
boxr resume s-1a2b3c-4d5e6f "now add the tests"
```

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh resume
```

Two paid sessions: the `headless-launch` drive, then

```
BOXR_HOME=<throwaway>/home boxr resume <id> "Reply with the single word yes and nothing else."
BOXR_HOME=<throwaway>/home boxr show <continuation id>
BOXR_HOME=<throwaway>/home boxr show <id>
```

## End state

- `launch.txt` is a normal `headless-launch` result.
- `resume.txt` exits 0 with `status: ok`, `ledger: recorded`, a new `id`, `resumedFrom: <id>`, the same `harnessSessionId` as `launch.txt`, and `steps` of at least 1.
- `show.txt` for the continuation carries `mode: resume` and `resumedFrom: <id>`.
- `show-origin.txt` for the original still carries `mode: headless`.
- `sessions/<id>/` and `sessions/<continuation id>/` each hold a non-empty `raw/stream.jsonl` and `raw/transcript.jsonl`.

## Gotchas

- Resume exits 0 once it has recorded the continuation, even when the harness turn failed, so the driver checks `status: ok` and not only the exit code.
- The continuation's `raw/transcript.jsonl` is the harness transcript after the resumed turn, so it also holds the original turn; the normalized layer holds only the new steps.
- Resume runs in the original session's `cwd` from `launch.json`, not the caller's directory.
- Claude Code needs the original transcript under its config directory to resume.
  A session whose `~/.claude/projects/...` file was cleaned up cannot be resumed.
