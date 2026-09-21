# remote launch

`boxr --remote <host>` forwards a headless launch to another machine over ssh.
It probes the remote boxr, starts a detached session on the host, prints the remote session id, and exits.
The session lives in the remote ledger only.

## Reach it

```
boxr --remote box-one --harness claude --model haiku "reply with the single word ok"
```

`--remote-dir <path>` starts the session in that remote directory.

## Drive it

```
BOXR_VERIFY_REMOTE_HOST=box-one .claude/skills/verify-boxr/scripts/verify-boxr.sh remote
```

The driver builds `target/release/boxr`, then runs this from a scratch directory against a throwaway `BOXR_HOME`:

```
boxr --remote "$BOXR_VERIFY_REMOTE_HOST" --remote-dir "/tmp/boxr-verify-<stamp>" \
  --harness claude --model haiku --effort low -- "Reply with the single word ok and nothing else."
```

The remote host needs boxr on `PATH` and a logged-in Claude Code, because the launch runs there.
The driver refuses to run without `BOXR_VERIFY_REMOTE_HOST`.

## End state

stdout is TOON with a `session:` section carrying `status: running`, `remote: <host>`, `harness`, `model`, and `dir:` set to the requested remote directory.
The printed id names the session on the remote host.

The throwaway `BOXR_HOME` on the local machine holds no `sessions/` directory, because a remote launch records nothing locally.
`ssh <host> boxr status <id>` reports the session from the remote ledger.

The evidence directory holds `toon.txt`, `stderr.txt`, `remote-status.txt`, `build.log`, and `meta.txt`.
`meta.txt` records the remote host, the remote version, the binary hash, the git revision, and whether the throwaway home was removed.

## Gotchas

- The remote host must run boxr with the same major and minor version as the local binary.
  A patch difference is accepted. Reinstall the remote boxr to match.
- A directory that does not exist on the remote host makes the `cd` fail before boxr starts, so boxr reports the remote launch as failed and prints no session id.
- boxr quotes `--remote-dir` for a POSIX shell.
  A space, a quote, or a metacharacter in the name is literal and never runs as a command.
- The remote launch spends quota on the remote host and leaves a session in its ledger.
  The driver removes its remote directory, not the remote session.
- The remote harness runs with the remote login, so the remote host needs its own Claude Code login.
