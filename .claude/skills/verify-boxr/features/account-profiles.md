# account profiles

A profile is an isolated harness config directory boxr owns, at `accounts/<harness>/<name>` under the boxr home.
A launch with `--account <name>` points the harness at it through its config-dir override (`CLAUDE_CONFIG_DIR` for Claude Code, `PI_CODING_AGENT_DIR` for pi).
boxr never copies credentials and never touches the user's own harness config.

## Sub-features

- `boxr account add --harness <h> --name <n>`: runs the harness's own login (`claude auth login`) with its config dir set to the new profile; a failed first login removes the directory again.
- `boxr account list`: `accounts[N]{harness,name,dir}:`.
- `boxr account remove --harness <h> --name <n> --yes`: deletes the profile; without `--yes` it is a usage error and deletes nothing.
- `--account <name>` on a launch: resolves the profile or fails with exit 2 before any session is created, and records `profile` in the summary and `account` in the launch result.
- `defaults.account` in `config.json` is the fallback when `--account` is absent.

## Reach it

```
boxr account add --harness claude --name work
boxr account list
boxr --harness claude --model haiku --account work "reply with the single word ok"
boxr account remove --harness claude --name work --yes
```

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh account-profiles
```

Zero paid sessions.
`account add` needs a human at a browser, so the driver stands in for it: it creates the empty directory `accounts/claude/verify` in the throwaway home by hand, as verification scaffolding, which is exactly what `account add` leaves before the login writes into it.
A launch on that empty profile runs the real Claude Code with no login, so it fails before any API call.
That failure is the proof that boxr routed the harness into the profile instead of the ambient `~/.claude`, which is logged in.

```
BOXR_HOME=<throwaway>/home boxr account list
BOXR_HOME=<throwaway>/home boxr --harness claude --model haiku --account verify -- "<prompt>"
mkdir -p <throwaway>/home/accounts/claude/verify
BOXR_HOME=<throwaway>/home boxr account list
BOXR_HOME=<throwaway>/home boxr --harness claude --model haiku --effort low --account verify -- "<prompt>"
BOXR_HOME=<throwaway>/home boxr show <id>
BOXR_HOME=<throwaway>/home boxr account remove --harness claude --name verify
BOXR_HOME=<throwaway>/home boxr account remove --harness claude --name verify --yes
```

## End state

| evidence file | must hold |
|---|---|
| `accounts-empty.txt` | `accounts[0]` |
| `missing-profile.txt` | exit 2 recorded in `meta.txt`, and no session directory created |
| `accounts-listed.txt` | the row `claude,verify,<dir>` |
| `launch.txt` | a non-zero exit, a session `id` and `account: verify` |
| `profile-contents.txt` | the files Claude Code wrote into the empty profile |
| `show.txt` | `profile: verify` |
| `remove-unconfirmed.txt` | exit 2, and the profile still exists |
| `remove.txt` | `status: removed`, and the profile is gone |

## Gotchas

- The doctor refuses to run this driver when `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN` is set, because Claude Code would log in from the environment, the launch would succeed, and it would spend quota.
- A logged-in profile launch is not driven.
  Proving it means a human runs `boxr account add --harness claude --name verify` against a throwaway `BOXR_HOME`, completes the login, and then runs the `headless-launch` command with `--account verify` from the same shell.
  Its transcript then lands under `<throwaway>/home/accounts/claude/verify/projects/`, not `~/.claude/projects/`.
- pi profiles use `PI_CODING_AGENT_DIR`; no driver covers them.
