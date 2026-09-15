# headless launch

Headless is boxr's default mode.
boxr starts the harness non-interactively, feeds it the prompt on stdin, follows its stdout stream, copies the harness's own transcript into the ledger, and prints a TOON result.

## Reach it

```
boxr --harness claude --model haiku "reply with the single word ok"
```

There is no flag for headless; it is what happens with no mode flag.

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh headless-launch
```

The driver builds `target/release/boxr` and then runs this from a scratch directory inside the throwaway home:

```
BOXR_HOME=<throwaway>/home boxr --harness claude --model haiku --effort low \
  -- "Reply with the single word ok and nothing else."
```

That is one real Claude Code session on the machine's normal login.

## End state

stdout is TOON with a `session:` section carrying `status: ok`, `harness: claude`, `model: haiku`, a boxr `id` of the form `s-<hex>-<hex>`, `harnessSessionId` set to a UUID, `durationMs`, `exitCode: 0`, and a `raw:` section carrying `ledger: recorded` plus the `transcript` path.

The process exit code is 0.

Then `sessions/<boxr id>/raw/` under the throwaway home holds:

| file | what it must be |
|---|---|
| `stream.jsonl` | the harness's stdout verbatim, one JSON event per line, ending in a `result` event |
| `transcript.jsonl` | a byte-identical copy of the harness's own transcript for that session |
| `stderr.log` | the harness's stderr, empty on a clean run |

The evidence directory holds `toon.txt`, `stderr.txt`, `build.log`, `meta.txt` and `raw/` with those same three files.
`meta.txt` records the binary hash, the git revision, the harness version, the config directory used, the boxr pid, the exit code, whether it timed out, the boxr session id, the harness session id and whether the throwaway home was removed.

## Gotchas

- Claude Code leaves its transcript under `~/.claude/projects/<slug-of-cwd>/<session id>.jsonl`.
  The driver runs from a scratch directory, so that slug belongs to the throwaway run and not to the repo.
  `meta.txt` records the exact path.
- boxr sends the prompt to the harness on stdin, not on the command line, so a long prompt or one holding quotes and newlines is fine.
  Put `--` before a prompt that starts with a dash so boxr's own parser does not read it as a flag.
- `--effort low` is accepted by Claude Code and refused by models that do not support effort.
  Drop the flag with `BOXR_VERIFY_EFFORT=` when a model rejects it.
- Claude Code returns quickly for a one-word prompt, but a cold start can take a few seconds.
  A run that takes minutes is a hang, not a slow model.
- Success is the exit code, not the word `ok` in the output.
  A ledger failure prints `status: ok` and exits 5, and a harness failure prints `status: failed` and exits 1.