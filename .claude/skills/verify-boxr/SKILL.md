---
name: verify-boxr
description: Use when proving boxr works against the real harnesses instead of the fakes the black-box suite uses. Covers boxr's headless launch (`boxr --harness claude --model <m> --effort <e> "<prompt>"`) against a throwaway `BOXR_HOME` and the raw ledger it records there, the outcomes surface (`boxr outcome <id> success --note <text>` and `boxr outcome --check-reverted <id>`) that reads and updates the summary ledger, and the session cost surface (`currency` and `prices` in config, `apiEquivalentCost` and `currency` in the launch output, `show` and `stats`). Later surface is added to this skill by the tickets that build it. Run it before a release, after changing a harness adapter, the launcher, the ledger writer or the outcome commands, or when a harness changes its output format.
---

# verify-boxr

## What this proves

The black-box suite replaces Claude Code, Codex and pi with fake executables, so it proves boxr's plumbing and never proves boxr still matches the real tools.
This skill drives the real harness in a throwaway boxr home, saves what it produced, and leaves the real ledger untouched.

## This spends real quota

Every drive step starts a real paid session on a real subscription.
The fixed inputs are the smallest spend that still proves the feature: one session, the smallest model, a one-line prompt, no tools, no detach.
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
   Copies the TOON result and the raw ledger layers into the run's evidence directory.
   `meta.txt` is written before the drive step with the doctor facts, and the boxr pid, exit code, timeout, session ids and cleanup result are appended as they become known, so a failing run keeps it too.
4. Cleanup.
   Deletes the throwaway home, asserts it is gone, and asserts the evidence is still there.

Exit code 0 means every step held.
Any other exit code names the step that failed and leaves the evidence behind.

## Feature map

`features/` has one file per user-facing feature that exists today.
Read the file for the feature you are proving.
It carries how to reach the feature, how to drive it, the end state that proves it, and its gotchas.

- [headless-launch](features/headless-launch.md): `boxr --harness claude --model <m> "<prompt>"`, the default blocking mode.
- [outcomes](features/outcomes.md): `boxr outcome <id> success --note "<text>"` and `boxr outcome --check-reverted <id>`.
- [session-cost](features/session-cost.md): `currency` and `prices` in `config.json`, recorded in the launch output and read back by `boxr show` and `boxr stats`.

A ticket that adds user-facing surface adds its own feature file, wires a driver into `scripts/verify-boxr.sh`, and extends the frontmatter description as part of its own work.
`features/README.md` states what a feature file must contain.

## Fixed inputs

The driver takes no switches.
It always drives Claude Code on `haiku` at `--effort low` with a fixed one-line no-tool prompt, kills the session after 300 seconds, and writes evidence under `~/.boxr-verify`.
The `session-cost` drive additionally writes the price table from its feature file into the throwaway home before the launch.

## Gotchas

- Claude Code writes its own transcript under its config directory, so a run leaves one transcript behind in `~/.claude/projects/`.
  boxr copies it into the throwaway home, and `meta.txt` records the path it came from.
  Account profiles (#3) remove that trace by giving the run its own config directory.
- The run uses the machine's normal harness login, because account profiles do not exist yet.
  The doctor step reads the same ambient config directory boxr will use, and refuses to run when it holds no login.
- A harness that is missing from `PATH` exits 3 and creates no session.
  That is a doctor failure, not a drive failure.
- A ledger failure exits 5 while `status: ok` still appears, so check the exit code and not just the TOON body.
- A timeout or an interrupt kills the whole process group when `setsid` exists, and only boxr otherwise, so a killed run without `setsid` can leave an orphaned harness process.
  Find it as a child of the `boxrPid` recorded in `meta.txt`, never by process name.