# outcomes

Every finished session records what happened to it as separate signals, never one rolled-up status: the exit facts (exit code and interruption always, plus the harness error and limit hit when the harness reports them) and the git evidence for the working directory.
On top of those, `boxr outcome` records the caller's own verdict and re-checks whether the recorded commits survived.

## Reach it

```
boxr outcome s-1a2b3c-4d5e6f failed --note "tests still red"
boxr outcome --check-reverted s-1a2b3c-4d5e6f
```

The verdict is one of `success`, `partial` or `failed`; the revert check takes no verdict and no note.
`boxr show <id>` folds the recorded signals into one session view, and `boxr stats --by verdict` groups sessions by them.

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh outcomes
```

The driver builds `target/release/boxr`, makes `<throwaway>/work` a git repository with one empty baseline commit, and then runs the same real session `headless-launch` runs:

```
BOXR_HOME=<throwaway>/home boxr --harness claude --model haiku --effort low \
  -- "Reply with the single word ok and nothing else."
```

Then it drives the outcomes surface against that session:

```
BOXR_HOME=<throwaway>/home boxr outcome <id> success --note verified
BOXR_HOME=<throwaway>/home boxr show <id>
BOXR_HOME=<throwaway>/home boxr stats --by verdict --since 7d
BOXR_HOME=<throwaway>/home boxr outcome --check-reverted <id>
```

That is one real Claude Code session on the machine's normal login; the four outcome commands cost nothing.

## End state

The launch prints the `session:` result with `exitCode: 0` and a `git:` section carrying `repo` and `commits: 0`, because the baseline commit is both the launch `HEAD` and the exit `HEAD`.

`outcome` prints the `outcome:` section:

```
outcome:
  id: <boxr id>
  verdict: success
  note: verified
help[2]:
  ...
```

`show` prints the same session with the verdict folded in, so the `session:` section carries `verdict: success` and `verdictNote: verified`.

`stats` prints `stats[1]{verdict,sessions,tokens,durationMs}:` with one `success,1,...` row.

`outcome --check-reverted` prints the `reverts:` section:

```
reverts:
  id: <boxr id>
  commits: 0
reverted[0]:
unknown[0]:
help[2]:
  ...
```

`<throwaway>/home/summary.jsonl` holds three records: the full summary the launch appended, the field-scoped record `outcome` appended carrying `verdict` and `verdictNote`, and the field-scoped record `--check-reverted` appended carrying `git`.
Nothing earlier is rewritten.

The evidence directory holds `toon.txt`, `stderr.txt`, `build.log`, `meta.txt` and `raw/` as for a headless launch, plus `outcome.txt`, `show.txt`, `stats.txt` and `reverts.txt`.
`meta.txt` also records `gitBase`, the baseline commit the session started from.

## Gotchas

- The real session is told not to use tools, so it commits nothing: the recorded commit set is empty and the revert check reports `commits: 0`.
  Proving the reverted path needs a commit a session actually made, which is the black-box suite's job in `tests/outcomes.rs`.
- `boxr outcome --check-reverted` needs git evidence, so a session launched outside a git repository has none and the check fails as a usage error.
  The driver avoids that by making the scratch directory a repository before the launch.
- `--check-reverted` is conservative: a commit still reachable from any local branch is left under `unknown`, never reported reverted, and a repository that has gone missing fails the check instead of reporting everything reverted.
- A note belongs to the verdict it was recorded with, so a later verdict recorded without `--note` clears the displayed note.
  The ledger keeps the earlier record either way, so `summary.jsonl` still shows which verdict that note justified.
- The verdict update and the revert check append to the same `summary.jsonl` without rewriting it, so a session that recorded both has three records and every reader folds them in order.
