---
name: verify-boxr
description: Use when proving boxr works against the real harnesses instead of the fakes the black-box suite uses. Covers boxr's launch surface (`--harness`, `--model`, `--effort`, `--account`, `--kind`, `--interactive`, `--detach`), the ledger under the boxr home, and the `show`, `export`, `account`, `stats`, `prompts`, `eval` and `skill install` commands. Run it before a release, after changing a harness adapter, the launcher or the ledger writer, or when a harness changes its output format.
---

# verify-boxr

## What this proves

The black-box suite replaces Claude Code, Codex and pi with fake executables, so it proves boxr's plumbing and never proves boxr still matches the real tools.
This skill drives the real harness in a throwaway boxr home, saves what it produced, and leaves the real ledger untouched.

## This spends real quota

Every drive step starts a real paid session on a real subscription.
The defaults are the smallest spend that still proves the feature: one session, the smallest model, a one-line prompt, no tools, no detach.
Do not loop this skill.
Do not run it in CI.
Run it once per change you need to prove.

## Isolation

- The drive step runs against a throwaway boxr home created under the system temp directory, so the real `~/.boxr` ledger and any profiles are never read or written.
- The script refuses to run if `BOXR_HOME` is already set, because that means it was launched inside a boxr home.
- Evidence is written to `~/.boxr-verify/runs/<run-id>/`, which is outside both the throwaway home and the repo, so it survives cleanup.
- Cleanup removes the throwaway home, by exact path, and any process the run started, by pid.
  Nothing is ever matched or killed by process name.

## The loop

Run the driver from the repo root:

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh <feature>
```

It runs four steps in order.

1. Doctor, read-only.
   Builds the release binary so the binary under test is the commit under test, checks that binary runs, records the git revision, the binary hash and whether the checkout is dirty, checks the harness is on `PATH`, and checks the harness config directory the run will use is logged in.
   It refuses to spend quota when login is missing.
2. Drive.
   Runs one real headless session against the throwaway home.
3. Evidence.
   Copies the TOON result and the raw ledger layers into the run's evidence directory and writes `meta.txt` with the doctor facts, the boxr session id and the harness session id.
4. Cleanup.
   Deletes the throwaway home, asserts it is gone, and asserts the evidence is still there.

Exit code 0 means every step held.
Any other exit code names the step that failed and leaves the evidence behind.

## Feature map

`features/` has one file per user-facing feature that exists today.
Read the file for the feature you are proving.
It carries how to reach the feature, how to drive it, the end state that proves it, and its gotchas.

- [headless-launch](features/headless-launch.md): `boxr --harness claude --model <m> "<prompt>"`, the default blocking mode.

A ticket that adds user-facing surface adds its own feature file and wires a driver into `scripts/verify-boxr.sh` as part of its own work.
`features/README.md` states what a feature file must contain.

## Switches

The driver reads these environment variables.

| variable | default | meaning |
|---|---|---|
| `BOXR_VERIFY_HARNESS` | `claude` | harness to drive |
| `BOXR_VERIFY_MODEL` | `haiku` | model to drive, keep it the cheapest that works |
| `BOXR_VERIFY_EFFORT` | `low` | effort level, empty string drops the flag |
| `BOXR_VERIFY_PROMPT` | a one-line no-tool prompt | the prompt to send |
| `BOXR_VERIFY_TIMEOUT` | `300` | seconds before the session is killed |
| `BOXR_VERIFY_EVIDENCE` | `~/.boxr-verify` | evidence root |

## Gotchas

- Claude Code writes its own transcript under its config directory, so a run leaves one transcript behind in `~/.claude/projects/`.
  boxr copies it into the throwaway home, and `meta.txt` records the path it came from.
  Account profiles (#3) remove that trace by giving the run its own config directory.
- The run uses the machine's normal harness login, because account profiles do not exist yet.
  The doctor step reads the same ambient config directory boxr will use, and refuses to run when it holds no login.
- Removing `--effort` matters for models that reject it. Set `BOXR_VERIFY_EFFORT=` to drop the flag.
- A harness that is missing from `PATH` exits 3 and creates no session.
  That is a doctor failure, not a drive failure.
- A ledger failure exits 5 while `status: ok` still appears, so check the exit code and not just the TOON body.
- The timeout path kills the whole process group when `setsid` exists, and only boxr otherwise, so a killed run can leave an orphaned harness process.
  Find it by the session id recorded in the run directory, never by process name.