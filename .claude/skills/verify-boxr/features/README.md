# Feature map

One file per user-facing feature that exists today.
Each file says:

- **Reach it**: the exact command a user or agent types.
- **Drive it**: the cheapest real run that exercises the feature, and the driver that runs it.
- **End state**: what stdout, the throwaway boxr home and the evidence directory must contain when it worked.
- **Gotchas**: what wastes quota, what leaves traces behind, and what fails misleadingly.

A feature file is also the claim that the feature exists.
Do not write one for something that is still being built.
Add the file in the same ticket that adds the feature, and wire its driver into `../scripts/verify-boxr.sh`.

| feature | command surface | driver | file |
|---|---|---|---|
| headless launch | `boxr --harness claude --model <m> "<prompt>"` | `headless-launch` | [headless-launch.md](headless-launch.md) |
| detached sessions | `--detach`, `boxr ps`, `status`, `tail`, `wait`, `stop` | `detached` | [detached.md](detached.md) |
| resume | `boxr resume <id> "<prompt>"` | `resume` | [resume.md](resume.md) |
| account profiles | `boxr account add\|list\|remove`, `--account <name>` | `account-profiles` | [account-profiles.md](account-profiles.md) |
| ledger reads | `boxr show`, `boxr export --atif`, `boxr stats`, `--kind` | `detached`, `outcomes`, `session-cost` | [ledger-reads.md](ledger-reads.md) |
| models and list | `boxr models --harness pi`, `boxr list [--all] [--limit N]` | `models-and-list` | [models-and-list.md](models-and-list.md) |
| outcomes | `boxr outcome <id> success --note "<text>"`, `--check-reverted` | `outcomes` | [outcomes.md](outcomes.md) |
| remote launch | `boxr --remote <host> [--remote-dir <path>] ...` | `remote` | [remote.md](remote.md) |
| serve | `boxr serve [--bind <IP>] [--port N] [--token <secret>]` | `serve` | [serve.md](serve.md) |
| session cost | `currency` and `prices` in `config.json` | `session-cost` | [session-cost.md](session-cost.md) |

## Proof status

A driver counts as proven once a run of it exited 0 and its evidence is under `~/.boxr-verify/runs/`.

| driver | last proven run |
|---|---|
| `headless-launch` | `20260915T145857Z-headless-launch` |
| `session-cost` | `20260917T224959Z-session-cost` |
| `detached` | `20260921T154324Z-detached` |
| `outcomes` | `20260921T162241Z-outcomes` |
| `resume` | `20260921T162228Z-resume` |
| `account-profiles` | `20260921T161215Z-account-profiles`, exit 0; logged-in launch proven by hand |

The `headless-launch` and `session-cost` runs above used the previous script, which kept the launch output as `toon.txt` and the raw layers at `raw/`; the drive itself is unchanged.

## Not driven by this skill

This surface exists and has no driver yet.
Its only proof is the black-box suite, against fake harnesses.

- `--harness pi`: the second launch adapter, proven by `tests/pi.rs` and by the pi dogfood in `docs/dogfood-report.md`.
  Every driver here is Claude-only.
- `boxr account add`: runs the harness's own interactive login, so it needs a human (see [account-profiles.md](account-profiles.md)).
- `boxr stop` on a running session: `detached` stops only a finished one, because stopping the live one would lose the `ok` result the rest of the drive needs.
  The dogfood proved the running path against pi.
- `boxr skill install --harness <h>`: writes into the real harness config directories, so it is never driven from here.
- Interactive mode (`--interactive`) is in `docs/design.md` but not in the CLI yet, so it has no file.
