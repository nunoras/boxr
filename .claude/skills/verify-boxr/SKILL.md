---
name: verify-boxr
description: Use when proving boxr works against the real harnesses instead of the fakes the black-box suite uses. Covers boxr's headless launch (`boxr --harness claude --model <m> --effort <e> "<prompt>"`) against a throwaway `BOXR_HOME` and the raw ledger it records there, detached sessions (`--detach` with `ps`, `status`, `tail`, `wait`, `stop`), `boxr resume`, account profiles (`account list/remove`, `--account` routing), the ledger readers (`boxr models --harness <h>`, `boxr list [--all] [--limit N]`, `boxr show`, `boxr export --atif`, `boxr stats`), declared kinds, the outcomes surface (`boxr outcome <id> success --note <text>` and `boxr outcome --check-reverted <id>`) that reads and updates the summary ledger, the session cost surface (`currency` and `prices` in config, `apiEquivalentCost` and `currency` in the launch output, `show` and `stats`), the read-only HTTP surface (`boxr serve --bind --port --token`), and the remote launch surface (`boxr --remote <host> --remote-dir <path>`). Later surface is added to this skill by the tickets that build it. Run it before a release, after changing a harness adapter, the launcher, the supervisor, the ledger writer, the serve listener or the outcome commands, or when a harness changes its output format.
---

# verify-boxr

## What this proves

The black-box suite replaces Claude Code and pi with fake executables, so it proves boxr's plumbing and never proves boxr still matches the real tools.
This skill drives the real harness in a throwaway boxr home, saves what it produced, and leaves the real ledger untouched.

## This spends real quota

Most drives start a real paid session on a real subscription.
The fixed inputs are the smallest spend that still proves the feature: the smallest model, a one-line prompt, no tools.
Each driver proves as much as it can from one session.

| driver | paid sessions |
|---|---|
| `headless-launch` | 1 |
| `outcomes` | 1 |
| `session-cost` | 1 |
| `detached` | 1 |
| `resume` | 2 |
| `account-profiles` | 0 |

Do not loop this skill.
Do not run it in CI.
Run the one driver that covers the change you need to prove.

## Isolation

- Every drive runs against a throwaway boxr home under the system temp directory, so the real `~/.boxr` ledger and its profiles are never read or written.
- Other tools (depot, firstmate workers) may be running real boxr sessions on the same machine against `~/.boxr`.
  They are invisible to the throwaway home, and the driver never signals them.
- The script refuses to run if `BOXR_HOME` is already set, because that means it was launched inside a boxr home.
- Evidence is written to `~/.boxr-verify/runs/<run-id>/`, outside both the throwaway home and the repo, so it survives cleanup.
- Cleanup removes the throwaway home by exact path, and only the processes the run started, by pid: the bounded boxr command it was waiting on and, for `detached`, the supervisor pid that `--detach` printed.
  Nothing is ever matched or killed by process name.

## Launch

There is no server to keep alive.
The driver builds `target/release/boxr` from the checkout it lives in, so the binary under test is the commit under test.
Never drive `~/.local/bin/boxr` or any installed copy.

Run the driver from the repo root:

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh <feature>
```

`<feature>` is one of `headless-launch` (the default), `outcomes`, `session-cost`, `detached`, `resume` or `account-profiles`.

## The loop

The driver runs four steps in order.

1. Doctor, read-only.
   Builds the release binary, checks it runs, records the git revision, the binary hash and whether the checkout is dirty, checks `claude` is on `PATH`, and checks the Claude config directory the run will use is logged in.
   It refuses to spend quota when login is missing.
   For `account-profiles` it instead refuses when `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN` is set, because then the empty profile would log in anyway and spend quota.
2. Drive.
   Runs the feature's commands against the throwaway home, each bounded by 300 seconds.
3. Evidence.
   Saves each command's stdout and stderr as `<label>.txt` and `<label>.stderr.txt`, and copies every session directory the run touched to `sessions/<id>/`.
   `meta.txt` is written before the drive with the doctor facts, and each command's pid, exit code and timeout, the session ids and the cleanup result are appended as they become known, so a failing run keeps them too.
4. Cleanup.
   Stops anything the run started that is still alive, deletes the throwaway home, asserts it is gone, and asserts `meta.txt` and `sessions/` are still in the evidence directory.

Exit code 0 means every step held.
Exit code 2 is a guard refusal before anything ran.
Any other non-zero exit prints `step:` and `why:` on stderr and leaves the evidence behind.

## Doctor, on its own

When a run looks off, check these by hand before driving again, all read-only:

```
cargo build --release && target/release/boxr --version
command -v claude && claude --version
test -f "${CLAUDE_CONFIG_DIR:-$HOME/.claude}/.credentials.json" && echo logged-in
ls ~/.boxr-verify/runs | tail -n 3
```

A stranded throwaway home shows up as `${TMPDIR:-/tmp}/boxr-verify.*`.
Its `meta.txt` in the matching run directory names the pids the run started, so check those before removing anything.

## Feature map

`features/` has one file per user-facing feature that exists today, plus the index of what is still unproven.
Read the file for the feature you are proving.
It carries how to reach the feature, how to drive it, the end state that proves it, and its gotchas.

- [headless-launch](features/headless-launch.md): `boxr --harness claude --model <m> "<prompt>"`, the default blocking mode.
- [detached](features/detached.md): `--detach` plus `ps`, `status`, `tail`, `wait` and `stop`.
- [resume](features/resume.md): `boxr resume <id> "<prompt>"`.
- [account-profiles](features/account-profiles.md): `boxr account add|list|remove` and `--account`.
- [ledger-reads](features/ledger-reads.md): `boxr show`, `boxr export --atif`, `boxr stats`, and `--kind`.
- [models-and-list](features/models-and-list.md): `boxr models --harness <h>` and `boxr list [--all] [--limit N]`, both without launching anything.
- [outcomes](features/outcomes.md): `boxr outcome <id> success --note "<text>"` and `boxr outcome --check-reverted <id>`.
- [remote](features/remote.md): `boxr --remote <host> [--remote-dir <path>]`, a detached launch on another machine over ssh.
- [serve](features/serve.md): `boxr serve [--bind <IP>] [--port N] [--token <secret>]`, the read-only HTTP view, driven without launching a harness.
- [session-cost](features/session-cost.md): `currency` and `prices` in `config.json`, recorded in the launch output and read back by `boxr show` and `boxr stats`.

A ticket that adds user-facing surface adds its own feature file, wires a driver into `scripts/verify-boxr.sh`, and extends the frontmatter description as part of its own work.
`features/README.md` states what a feature file must contain and lists the surface nobody has driven yet.

## Fixed inputs

The driver takes no switches.
When the feature drives a harness it runs Claude Code on `haiku` at `--effort low` with a fixed one-line no-tool prompt, kills the session after 300 seconds, and writes evidence under `~/.boxr-verify`.
`resume` continues with `Reply with the single word yes and nothing else.`, `detached` declares `--kind describe`, and `account-profiles` uses a profile named `verify`.
The `models-and-list` drive launches no harness and spends no quota.
The `serve` drive launches no harness either.
The `remote` drive spends quota on the host named by `BOXR_VERIFY_REMOTE_HOST` instead of on the local machine.
The `session-cost` drive additionally writes the price table from its feature file into the throwaway home before the launch.

## Gotchas

- A run without a profile uses the machine's normal Claude login, so Claude Code writes its transcript under `~/.claude/projects/<slug-of-cwd>/` and a run leaves one transcript there per paid session.
  boxr copies it into the throwaway home, and `meta.txt` records the path it came from as `harnessTranscriptSource`.
  A logged-in boxr profile would avoid that trace, but logging one in is interactive, so no driver does it (see [account-profiles](features/account-profiles.md)).
- A harness that is missing from `PATH` exits 3 and creates no session.
  That is a doctor failure, not a drive failure.
- A blocking launch that hits a ledger failure exits 5 while `status: ok` still appears, so check the exit code and not just the TOON body.
  `wait`, `status`, `ps` and `resume` exit 0 for every turn outcome; read `status:` for the outcome.
- A bounded command that times out, or an interrupt, kills its whole process group when `setsid` exists, and only that pid otherwise, so a killed run without `setsid` can leave an orphaned harness process.
  Find it as a child of the pid recorded in `meta.txt`, never by process name.
- A detached session's supervisor lives in its own process session, outside the driver's group.
  The driver tracks it by the `pid:` that `--detach` printed and kills that pid group in cleanup if it is still alive.
