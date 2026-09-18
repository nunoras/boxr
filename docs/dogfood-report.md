# boxr dogfood-2 report

Harness lane: pi via openrouter/google/gemini-2.5-flash-lite (cheap) and opencode-go/grok-4.6 (limit probe).
Claude/Codex/OpenCode-Go weekly exhausted; xai also worked in a direct probe.
Isolated BOXR_HOME under /tmp/fm-boxr-dogfood-2/.
Binary under test: worktree release build on fm/boxr-dogfood-2.

## Exercised and held

- Blocking headless launch lands in the ledger; exit facts match harness stream/transcript on success (status ok, message ok, exitCode 0, tokens from usage).
- Detached launch through ps, status, wait (finish), tail.
- stop on a still-running session: action stopped, status interrupted, exitCode 137.
- Killed supervisor (SIGKILL): status interrupted, exitCode 137, summary folded on first status read. report.json is not written on this path (summary is); show/status still work.
- resume continues the original pi harness transcript (same --session-id and origin --session-dir), records resumedFrom, keeps the origin harnessSessionId, and normalizes only the new steps.
- show, export --atif, stats --by harness,model,status --since 7d, outcome success --note.
- Failed turn / usage limit: after fix, status failed, limitHit true, error from pi errorMessage, boxr process exit 1 even when pi exitCode is 0.
- Expired wait --timeout 1 while still running: status running, exit 0.

## Broke, then fixed in this branch

1. pi adapter ignored stream errors/limits. A real 429 GoUsageLimitError was recorded as status ok / error null / limitHit false. Fix: parse stopReason/errorMessage in final_message; treat reported error or limit as failed even when the process exits 0; Report::succeeded follows status.
2. pi resume started a fresh session id/dir, so the continuation did not append to the origin transcript and normalized steps stayed 0. Fix: resume Launch keeps the origin harness dir + original session id (walking `resumedFrom` for multi-hop); fake-pi appends when the session file already exists.

## Left untried / limits of this dogfood

- Real Claude Code and Codex launches (quota exhausted).
- Account profiles / config_dir isolation against a real harness login.
- outcome --check-reverted (no real commits from the throwaway work repo).
- Long in-tool sleep stop mid-bash: flash-lite either finished fast or failed the tool turn before sleep 25 held; stop was still proven by stopping immediately after detach.
- supervisor.json concurrent-reader race: write path already uses write_file_atomically; not stress-tested under load.
- Memory pressure / multi-session concurrency (host cap; one session at a time by brief).

## Evidence

- /tmp/fm-boxr-dogfood-2/evidence/
- /tmp/fm-boxr-dogfood-2/home-fixed/ (post-fix ledger)
