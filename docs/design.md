# boxr design

Decisions settled in the design grilling on 2026-09-15.
Each section is a decision, not a proposal.

## What boxr is

boxr is two products in one CLI.
The launcher starts any coding agent on any harness, model, effort and account.
The ledger records every session, classifies the work, and turns the records into stats, evals and prompt improvements.

It follows the axi style: agent-first, short TOON output, and `help[]` lines that suggest the next command.

Written in Rust.
The tool is open source under Apache-2.0.
All data is private and lives only in the boxr home, `~/.boxr` (or `%USERPROFILE%\.boxr` on Windows) unless the `BOXR_HOME` environment variable points elsewhere, never in the repo.
There is no sync and no telemetry.

## Platforms

Linux and Windows native are first-class, with CI on both from the first milestone.
macOS is best-effort.

## Launching

```
boxr --harness <h> --model <m> --effort <e> --account <profile> [--kind <k>] "<prompt>"
```

Harness, model, effort and account are explicit, with defaults from config.
A single bare word with no launch flag is refused as an unknown command, naming the closest command, so a mistyped subcommand never starts a paid session by accident.
A multiword prompt, or a single-word prompt alongside `--harness`, `--model`, `--detach`, `--remote`, `--effort`, `--account` or `--kind`, still launches.

### Headless (default)

The harness runs non-interactively and boxr reads its structured event stream.
By default the call blocks and prints a short TOON result: status, session id, trimmed final message, tokens, cost, duration, and next-step `help[]` lines.
`--detach` returns the session id immediately and the session keeps running in the background under boxr.
Detached sessions are managed with `boxr ps`, `boxr wait <id>` (optional timeout), `boxr status <id>`, `boxr tail <id>` and `boxr stop <id>`.
`boxr --remote <host> ...` forwards a headless launch over ssh: it probes `boxr --version` on the host, requires the same version as the local binary, runs `boxr --detach` there with the same harness, model, effort, account and kind, prints the remote session id, and exits.
The session is recorded only in the remote ledger; there is no cross-machine store.
Follow-up reads go through the remote host (`ssh`, or `boxr serve` on that machine).
`boxr resume <id> "<prompt>"` continues a finished session.
If boxr crashes, the harness process is killed rather than orphaned, and the session is marked interrupted.

#### Resuming a finished session

`boxr resume <id> "<prompt>"` continues a finished session with a new prompt, using the harness, model, effort, profile and harness session id the original recorded, so the harness resumes its own conversation.
Claude Code is driven with `--resume <harness session id>` and the prompt on plain stdin, because one prompt continues one conversation; streaming input stays available for a later capability that sends follow-ups into a running session.
pi is driven with the original `--session-id` and the original `--session-dir` so it appends to the same harness transcript file.
Like a foreground launch, resume blocks until the continuation finishes and prints the same result shape.
The continuation is a new session with its own id, its own raw, normalized and summary records, and `mode: resume`, linked to the original through `resumedFrom` in the normalized header, the summary line and the printed result.
A multi-hop resume walks `resumedFrom` to the origin session so `--session-dir` and the transcript byte offset still point at that original harness directory, not an intermediate continuation's empty one.
Its ledger starts at the byte offset the harness transcript had already reached when the continuation launched, so the original steps are recorded once, in the original session, and the original session's files are never rewritten.
The launch record stores `mode`, `profile` and `resumedFrom` with the rest of the launch, so a supervisor killed before the report and summary are written still reconstructs an interrupted continuation linked to its parent.
Resuming an unknown session, a session that is still running, a session with no recorded harness session id, or one whose harness transcript is gone is a usage error that names which of those it was.
`boxr resume --detach <id> "<prompt>"` starts the continuation in the background the way a fresh detached launch does.
It prints the new child id in the detached result shape, with `resumedFrom` naming the parent.
The launch record it writes before spawning the supervisor carries the resume mode, the harness session id, the transcript byte offset and the parent, so a supervisor that starts later adopts the persisted continuation instead of launching fresh.

#### Retrying a limited session

`boxr retry <id> [--detach]` re-drives a finished session whose summary recorded `limitHit`.
It reuses the saved harness, model, effort, profile, cwd, kind and prompt, and runs exactly one continuation through the same path as `boxr resume`.
When the session recorded a harness session id, the retry resumes that conversation from the transcript byte offset; when it did not, the retry starts a fresh launch linked to the parent through `resumedFrom` rather than fabricating a transcript resume.
Retrying a session that is still running, one that did not hit a limit, or one without a launch record to read its saved prompt and cwd from is a usage error.
Retry never loops: it performs one continuation and stops, so a retry that hits the limit again is itself retryable and nothing re-drives it automatically.
The retry child records `mode: retry` and the parent stays exactly as it was.

#### Supervising a detached session

Every headless launch writes a launch record and its supervisor pid into the session directory, so a session can be observed while it runs.
`--detach` spawns a second boxr process into its own process session, which supervises the harness and writes the launch result and the summary line, while the launching boxr exits.
If detach fails after writing the launch record, any spawned supervisor is killed and reaped and the session is finalized as interrupted, so it does not stick as starting or keep running after the caller was told the launch failed.
`boxr ps` lists every session that is still starting or whose supervisor is still alive, foreground or detached.
A session stays running while its supervisor is alive, even if a summary line already exists; finish from the summary or report only when the supervisor is not alive.
A launch record without a supervisor pid is still starting, not interrupted.
Only when a supervisor record was written and its pid is dead, and the summary and report are still missing, is the session recorded as interrupted the first time `ps`, `status` or `wait` looks at it, so a killed supervisor still leaves one summarized session.
`boxr status` reports a running session without blocking and the launch result of a finished one, and `boxr wait` blocks until the result exists.
`boxr wait` exits zero whenever it can report an outcome, whether the turn ended `ok`, `failed` or `interrupted` or is still `running` because its timeout expired, and keeps a non-zero exit only when boxr cannot do its job at all: an unknown session, or records it cannot read.
A caller that reads a non-zero exit as a failure cannot otherwise tell a failed turn from a command that never ran, and a failed turn is an outcome to record rather than an error.
`boxr resume` reports a turn the same way, so it exits zero once it has recorded the continuation, including when the harness turn failed or the ledger could not be written.
A blocking launch is the one that differs: its process exit follows the turn outcome, not only the harness process exit code, because its contract is the harness result rather than a report about it.
It exits non-zero when the turn is `failed` or `interrupted`, including when the harness process still exited zero but reported an error or a limit, and exits zero only when the turn is `ok`.
`boxr show` names the captured stderr file in its help when a session did not succeed, the same way a failed launch does.
`boxr show` prints the final assistant message as `message.text`, truncated to 200 characters, and `boxr show --message <id>` prints the full text instead with no TOON wrapper and no truncation.
The message comes from the last agent step in the normalized ledger, falling back to the launch result when the ledger recorded none, so a session that wrote no final message reads `message.text: null` and `--message` prints nothing.
The shape of `boxr ps`, `boxr status` and `boxr wait` is a contract other tools read, so it is fixed: `boxr ps` prints `sessions[N]{id,state,harness,model}:` with one row per running session, `boxr status` prints `state:` from `running`, `finished`, `stopped`, `interrupted` and `failed`, and `boxr wait` prints `status:` from `ok`, `failed`, `interrupted` and `running`.
`status` is the turn outcome and `state` is the session lifecycle, so a finished session reads `state: finished` with `status: ok`.
A running session adds two fields to `status`, `wait` and `GET /status/<id>`: `lastActivity`, the most recent normalized step timestamp, and `currentTool`, the newest tool call the harness started and has not answered, or `null` when no call is in flight.
Both fields are additive, so `boxr ps` keeps its four columns and the existing `status` and `wait` fields do not move.
`lastActivity` falls back to the normalized file's modification time and then to the launch time, and `currentTool` reads the harness transcript when the held tool call has not reached the normalized ledger yet, so a long tool call does not read as a hang.
boxr's own lifecycle maps onto the five states rather than adding to them: a turn with status `ok` is `finished`, a harness failure or reported error/limit is `failed`, and a session ended from outside is `interrupted`.
`stopped` is never printed, because `boxr stop` ends a session from outside and that is already `interrupted`.
`boxr tail` streams the normalized ledger as it is appended.
`boxr stop` writes a stop file into the session directory instead of signalling the supervisor, so it works the same way on both platforms, and it reconciles the session itself if the supervisor does not answer within ten seconds.
A detached supervisor appends a `supervise start <id>` line and a `supervise finish <id> exitCode=<n>` or `supervise error <id> exitCode=<n>` line to `supervisor.log`, so a session whose supervisor never wrote a report still leaves a trace of how it ended.
The lifecycle lines carry the session id and the exit code and never the prompt or the saved session metadata.
`boxr serve [--bind <IP>] [--port N] [--token <secret>]` exposes the same read surface over HTTP as JSON so a fleet view on another machine can poll this host (default port 4035, default bind `127.0.0.1`).
`--bind` takes exactly one IPv4 or IPv6 address, so the server never starts on whatever a shell expansion happens to produce, and binding anywhere but loopback without a token prints a warning on stderr rather than failing, because the fleet view has a reason to be reachable.
The token comes from `BOXR_SERVE_TOKEN` when the flag is absent, because `--token` puts the secret in the process arguments, where `/proc/<pid>/cmdline` and the shell history can read it; the flag still wins when both are given.
With a token, every request must carry `Authorization: Bearer <secret>` and is refused with 401 before routing when it does not, so an unknown path and a mutation are as protected as a known one, and the refusal carries `WWW-Authenticate: Bearer`.
The server reads the request up to the blank line that ends the headers instead of a single fixed-size read, so an `Authorization` header that arrives in a later TCP segment is still honoured, and the secret never reaches the listen output or a log line.
The endpoints are `GET /ps`, `GET /status/<id>` and `GET /outcome/<id>`; anything that would mutate the ledger is refused with 405 once the request is authenticated.
The ledger is still written only by the launch and outcome paths on this machine.
The harness dies with boxr: on Linux it is given `PR_SET_PDEATHSIG`, and on Windows it joins a job object that kills it when boxr exits.
If that guard cannot be set up, the launch fails and any spawned harness is killed rather than left running unprotected.

### Interactive (`--interactive`)

boxr sets up the account environment and runs the harness TUI in the terminal it was called from.
On every platform the harness runs as a child process sharing that terminal, and boxr never exec-replaces itself.
boxr stays alive while the child runs so it can follow the transcript live and record the session, and it exits with the child's exit code.
boxr never manages terminals, panes or multiplexers.
Whatever owns the terminal (tmux, herdr, Orca, an IDE) is the caller's business.

## Harnesses

Milestone 1 supports Claude Code, Codex and pi.
Other harnesses are added later as separate adapters.

pi isolates through `PI_CODING_AGENT_DIR`, and `--session-id` with `--session-dir` let boxr choose the transcript file up front.
pi is multi-provider, and `--model provider/id` picks the provider.

Confirmed on 2026-09-16 against pi 0.85.1, its shipped CLI and a recorded session: `--session-dir` is the session storage directory itself, not a parent, and pi names the file `<ISO timestamp with colons and dots replaced by dashes>_<session-id>.jsonl` there.
So only the timestamp prefix is unknown before launch: boxr uses its own session id as pi's session id, owns the session directory, and finds exactly one file by its `_<session-id>.jsonl` suffix.
A session file is JSONL of `session`, `model_change`, `thinking_level_change`, `message` and `label` entries, where a `message` entry carries one `user`, `assistant` or `toolResult` message, and only the assistant message reports usage, the model and the provider.
Print mode reports no pi version, so the normalized header records the agent version as `unknown`.

A harness may expose its own model catalog, and `boxr models --harness <h> [--account <name>]` prints it as a `models[N]{provider,model}:` table.
boxr runs the harness's discovery command (for pi, `pi --list-models`), parses the provider and model columns behind the adapter, and never keeps a hardcoded model list or launches a model to discover an id.
A harness without discovery is a usage error that names the harness, so `boxr models --harness claude` says claude does not expose a catalog rather than printing an empty table.
For pi, a fresh launch checks the exact `provider/model` against the discovered catalog before the session directory is allocated or the harness is spawned, so an unknown id is a usage error that names the closest full id by deterministic edit distance and creates no session.
Resume and retry reuse the model the original session recorded, which that session's fresh launch already checked.
A discovery command that cannot run or exits non-zero fails the launch clearly, so boxr never passes an unvalidated model to a paid run.
The account's config directory is applied to the discovery command exactly as it is applied to a launch, so a profile sees its own catalog.

## Accounts: profiles and subscriptions

These are two separate concepts.

A profile is an isolated credential home per harness, owned by boxr, for example `~/.boxr/accounts/claude/work/`.
`boxr account add --harness <h> --name <n>` runs that harness's own login pointed at the isolated directory.
At launch boxr points the harness at the profile through its config-dir override.
boxr never copies or swaps credential files, so parallel sessions on different profiles work.
A harness without a config-dir override is not supported.

A subscription is what you pay for and whose limits you burn, for example `anthropic-max`, `chatgpt-plus` or `zai-coding`.
One profile can carry credentials for several subscriptions (a pi profile holding Anthropic and Z.AI logins).
boxr resolves the billed subscription from the profile and the model, and every session record stores both.
This keeps Claude Code usage and pi-on-Claude usage counted against the same plan.

## Ledger

Three layers per session:

1. Raw: the harness's own transcript and stream, copied verbatim and never edited.
   It is the audit trail and can be re-normalized when parsers improve.
2. Normalized: JSONL where each line is one [ATIF](https://www.harborframework.com/docs/agents/trajectory-format) step object, plus a header line (session and agent info) and a closing line (final metrics).
   Our own fields (profile, subscription, effort, kind, mode, resumedFrom) go in ATIF `extra`.
   `boxr export --atif <id>` wraps the lines into a standard ATIF document.
   ATIF requires at least one step, so exporting a session that recorded none is refused as a usage error rather than written as an invalid document.
3. Summary: append-only JSONL in the boxr home, one full record per session with harness, model, effort, profile, mode, resumedFrom, subscription, start, end, tokens, cost, status, kind and outcomes, plus later field-scoped records carrying only the fields a `boxr outcome` update changes; readers fold the records of one session in order rather than taking the last line.
   All analytics query this layer.
   Status is `ok` for a zero exit and `interrupted` when the harness was stopped from outside or by boxr itself: an external-stop signal (`SIGINT`, `SIGTERM`, `SIGHUP`, `SIGKILL`) on Unix, Ctrl-C or Ctrl-Break (`STATUS_CONTROL_C_EXIT`) on Windows, or a termination boxr caused on either platform.
   A crash signal such as `SIGSEGV` or `SIGABRT` is not an interruption.
   One narrow Windows exception: a forced kill by a third party (`taskkill /F`, `TerminateProcess`) exits with an ordinary code that cannot be told apart from a real failure, so it is recorded as `failed`.
   Every other non-zero exit is `failed`, with or without a final result.
   When boxr itself is asked to stop, it kills the harness, lets the transcript follower finish, writes the closing line and a summary marked `interrupted`, and only then exits.
   That includes closing the console, logoff and shutdown on Windows, where boxr holds the console control event until the summary is written, within Windows' five second budget for those events.

`boxr list [--all] [--limit N]` reads the folded summary ledger and prints a `sessions[N]{id,state,harness,model,status,start,durationMs,kind,verdict}:` table, newest first by `start`.
A session whose supervisor is still alive appears once, with `state: running` and `status: running`, even before its summary exists; every other row is `state: finished` with the recorded turn status and the verdict the outcome records folded in.
The default view shows the 20 most recent sessions.
`--all` drops the limit unless `--limit` is also given, in which case the explicit limit wins.

Storage is JSONL on disk.
TOON is only the output shape when an agent reads through the CLI.

### Live capture

The main path is tailing the harness's transcript file live from launch to exit.
Hooks are the backup, for a harness that does not flush while running or for events the transcript does not record, such as waiting on the user.
Hooks are installed inside boxr's own profile directories, never in the user's normal harness setup.
Importing old transcripts uses the same parsers.

### Tool calls in the normalized layer

Claude Code writes each content block of one model reply (thinking, text, each `tool_use`) as its own transcript line, and every one of those lines repeats the reply's usage.
boxr maps each line to one agent step but counts a reply's usage once, on the first step it appears on.
Claude Code redacts thinking to an empty body, so a line with no text, reasoning or tool calls produces no step, and the reply's usage lands on the step that carries its text or tool calls.
Tool results arrive later on separate `type: user` lines.
They cannot become their own steps: ATIF allows `observation` only on agent steps, and the Harbor validator rejects any observation whose `source_call_id` does not match a `tool_call_id` on the same step, so a later step cannot answer an earlier step's call.
So an agent step that carries tool calls is held back until the results for all of its calls have arrived, then appended once with the results folded in as its `observation`.
Steps without tool calls are appended immediately, and step ids follow append order.
Liveness lags by tool duration for held steps, and every appended line stays a valid ATIF step.
A folded result keeps the harness's own error flag as `is_error`, taken from pi's `isError` message field and Claude's `is_error` tool result part.
The field is written only when the harness stated it, so an explicit false and a missing flag stay distinct and no flag is inferred from the result content.
When the harness dies before a result arrives, the held step is appended without it before the closing line, so an interrupted session still has a complete, valid file.

### Secrets

The raw layer is stored verbatim with owner-only permissions (0600 files in a 0700 directory) and is never read by any model.
The stderr tail of a failed harness process is recorded in the report only, printed by `boxr status`, `boxr wait` and `boxr show`, and never written to the summary or served over HTTP.
The normalized and summary layers pass through redaction on write: known secret patterns (API key prefixes, JWTs, private key blocks, `KEY=value` env lines) plus user-listed values from config, replaced with `[REDACTED:<kind>]`.
Anything sent to a model (classification, eval mining, the prompt skill) reads only redacted layers.
The promise is "redacted where recognized", never "safe to share".

## Kinds

Fixed core: `build`, `fix`, `research`, `plan`, `review`, `chore`, `docs`, `describe`.
`describe` is a reviewer-facing pull request description written from a diff; it is not a build, fix, or docs turn.
Custom kinds are added in config, each with a one-line description the classifier can use.
One kind per session.

The caller declares a kind with `--kind`, recorded as `declared`.
Without one, a post-session pass with a cheap model assigns a kind, recorded as `inferred` with a confidence score.
Heuristics (no file edits, docs-only changes) feed hints into that pass and are not a separate system.

## Outcomes

Four separate fields, never blended into one score:

- Exit facts: exit code and interruption always, plus the harness error and limit hit when the harness reports them. Automatic. The claude and pi adapters both report an error or a limit when the harness stream carries one; a reported error or limit marks the session `failed` even when the harness process still exits zero. A harness process that exits non-zero without a structured error records the last 4096 bytes of its stderr, snapped forward to a line start and trimmed, in the report as `stderrTail`; `boxr status`, `boxr wait` and `boxr show` print it, and the same path is named in `boxr show` help for a failed session. The tail stays out of `summary.jsonl` and out of every `boxr serve` response, because the raw layer is owner-only. A structured error is richer, so it is never replaced, and a zero exit with stderr warnings stays `ok`. A harness that recovers, such as pi after a successful `auto_retry_end`, clears the pending error and limit so a later error is the one recorded; the transcript keeps every attempt either way.
- Caller verdict: `boxr outcome <id> success|partial|failed --note "..."`. Optional and the strongest signal. A note belongs to the verdict it was recorded with, so a later verdict recorded without `--note` clears the displayed note while the ledger keeps the earlier record.
- Git evidence: commits made, files changed, and later whether those commits were reverted, re-checked with `boxr outcome --check-reverted <id>`. Automatic.
- Inferred judgment: the post-session pass judges whether the task was finished. Labeled inferred.

Git evidence records commits reachable from the exit `HEAD` that were not reachable from the launch `HEAD`, so commits reset away before exit are not recorded.
The recorded set is an upper bound on the session's work in two cases: when the launch `HEAD` is absent while other refs already carry history, the whole history reachable from the exit `HEAD` is recorded, and when a session dies without writing its summary, the evidence is collected at the next reconciliation instead of at the exit `HEAD`.
`boxr stats` and `boxr show` can over-count in exactly those two cases.
A revert check marks a recorded commit reverted only when no local branch contains it; a commit still reachable from any local branch remains unknown.

## Cost

Three measures, each labeled:

- Raw tokens: input, output, cached, reasoning. Ground truth.
- API-equivalent cost: tokens times list price, the estimate that makes models comparable.
  The list prices come from `prices` in config, keyed by model, in the currency `currency` sets (`USD` or `EUR`), per million tokens for `input`, `output`, `cached` and `reasoning`.
  Cached input and reasoning tokens are subtotals of the prompt and completion counts the harness reports, so they are priced at their own rate and the rest at the input and output rates: `(prompt - cached) * input + cached * cached + (completion - reasoning) * output + reasoning * reasoning`.
  The reasoning rate maps whatever split the harness reports (Claude's `thinking_tokens`, pi's `reasoning`) onto the table, so a provider that bills thinking as ordinary output needs `reasoning` set equal to `output`.
  A model with no entry in the table records `apiEquivalentCost` as null rather than zero, so an unpriced session is never read as a free one.
  It also records a `costError` that names the model and a `costUnpriced` flag, so the missing price is visible in the output instead of silent.
  Prices are arithmetic on the table and nothing else; boxr never fetches a price.
- Quota share: the percentage of a subscription window consumed, from quota readings before and after, split by token share when sessions overlap. Always marked estimated.

## Stats

`boxr stats` is the only source of numbers, for example `boxr stats --by model,kind --since 7d`.
Each row carries the requested dimensions, the `currency` the cost is in, `sessions`, `tokens`, `durationMs`, the summed `apiEquivalentCost` and `unpricedSessions`, the count of sessions in that group whose model had no price.
A priced session whose arithmetic failed records a `costError` too, and stats does not count it as unpriced.
Cost is grouped by currency as well, because adding amounts from different currencies would mean nothing.
Every visual view (a Lavish report, a later dashboard) is built on its output rather than querying the ledger itself.
Stats is a direct pass over the summary JSONL: fold each session's records in order, filter by `--since`, group by the requested dimensions plus currency, and emit the TOON table.

## Evals

A scenario is a folder with:

- Starting point: a repo at a fixed commit, set up fresh in an isolated copy for each run.
- Task: the prompt, with a slot for the prompt variant under test.
- Checker: a script that exits pass or fail and can emit sub-scores. No LLM judge.
- Budget: time and token limits. Going over is a fail.

The score is a fixed formula: checker result first, then API-equivalent cost and time as weighted penalties from config.
Quota share is never used in scoring, because it depends on unrelated concurrent work.

The checker is deterministic but the agent is not.
Each variant runs N times (default 5), and variants are compared on pass rate and median cost, with `boxr eval compare` stating whether a difference is beyond run-to-run noise.

Scenarios are mined from real sessions.
A past session with a success verdict and a commit is a candidate: its parent commit is the starting point, its prompt is the task, and the tests added in its commit are the checker.
Candidates join a suite only with the user's approval.

## Prompt skill

The CLI does everything deterministic and the skill does the judgment.

CLI primitives:

- `boxr prompts --kind <k>` returns prompts from the redacted layer with their outcomes and costs.
- `boxr eval run --suite <s> --variant a.md --variant b.md --runs 5` runs scenarios and prints scores.
- `boxr eval compare` reports whether a difference is real.

The skill is a standard SKILL.md.
It reads the user's prompts, describes their typical style, proposes one specific change as a variant, measures it through the eval CLI, and reports the result.
Every claim it makes must come from CLI output.
It ships in this repo and installs with `boxr skill install --harness claude|codex|pi|all`.

## Skills

Bundled skills live under `skills/<name>/` in this repo and are embedded in the binary at build time.
`boxr skill install --harness claude|codex|pi|all` writes each bundled skill into the selected harness's user-level skill directory.
A reinstall deletes the skill directory and writes it again, so an update leaves no stale files and no duplicates.
The bundled list in `src/skill.rs` is a list of named skills, each with its own files, and each installs into its own `skills/<name>/` directory, so skills cannot overwrite each other.
A new bundled skill is one more `skills/<name>/` folder plus one named entry in that list, and it needs no adapter: only the harness's skill directory, not its launch adapter.
Repo-local project skills, such as `verify-boxr` under `.claude/skills/`, are not bundled and are not installed by this command.

Each harness resolves its config directory from its own override environment variable, falling back to the user home, and each skill sits under `skills/` inside it.

| harness | config dir override | default config dir | user skill directory |
|---|---|---|---|
| Claude Code | `CLAUDE_CONFIG_DIR` | `~/.claude` | `<config>/skills/<name>` |
| Codex | `CODEX_HOME` | `~/.codex` | `<config>/skills/<name>` |
| pi | `PI_CODING_AGENT_DIR` | `~/.pi/agent` | `<config>/skills/<name>` |

Confirmed on 2026-09-15 from each tool's own docs or shipped files: Claude Code documents personal skills at `~/.claude/skills/<name>/SKILL.md`, Codex's shipped `skill-installer` skill names `$CODEX_HOME/skills` with default `~/.codex/skills`, and pi documents global skills at `~/.pi/agent/skills/`.

## Verification

Two layers prove boxr works.

The automated suite drives the `boxr` binary as a black box against a throwaway boxr home, with fake harness executables standing in for Claude Code, Codex and pi.
It proves boxr's own behaviour and says nothing about harness format drift.

The spec's opt-in smoke suite is the project-local `verify-boxr` skill under `.claude/skills/verify-boxr/`.
It is run by hand and never in CI.
It builds the binary, drives a real harness in a throwaway boxr home so the real ledger and profiles are never touched, saves the TOON result and the raw ledger layers to an evidence directory that outlives the run, and then removes only what the run created.
Its `features/` map holds one file per user-facing feature, and each ticket that adds user-facing surface adds its own file there.
A run spends real subscription quota, so the skill always uses the cheapest model and a one-line prompt.

## Milestones

1. Launch and ledger: headless and interactive, isolated profiles, subscriptions, live tailing into raw, ATIF JSONL and summary layers, redaction.
   Claude Code, Codex and pi.
   Done means every session launched through boxr is recorded live.
2. Stats: `--kind`, `boxr outcome`, the inferred kind and outcome pass, `boxr stats` over the summary ledger, and import of existing transcripts.
3. Evals: scenario format, runner, scoring, noise comparison, and scenario mining with approval.
4. Prompt skill.

More harness adapters can land between milestones.
Firstmate adopting boxr as its launcher is a separate project after milestone 1 is solid.

## Deferred

Routing advice.
Once stats and evals exist, boxr can suggest changes to an existing routing setup (which harness, model and effort to use per kind), backed by its evidence and quota headroom, and explain why.
boxr never routes on its own.
This is a later milestone, not part of 1 to 4.

## Open questions

- Is the `boxr` name free on crates.io, and is the `boxr.sh` domain available?
  Other projects named boxr exist (a Ruby Box API client, an R package, a small container engine), none in the agent space.
- How does Codex handle an isolated config directory and a caller-chosen session id?
- Does Claude Code flush its transcript live in headless mode as well as interactive?

## Prior art

- [agentic-coding-harness](https://github.com/sblattj/agentic-coding-harness): drives several agent CLIs from outside with unified events, token records and ATIF trajectories.
- [oneharness](https://github.com/nickderobertis/oneharness): one CLI across harnesses with uniform JSON output.
- [caam](https://github.com/Dicklesworthstone/coding_agent_account_manager): fast subscription account switching for coding CLIs.
- [Arize coding-harness-tracing](https://arize.com/blog/open-source-coding-agent-tracing/): tracing for Claude Code, Codex, Cursor and Gemini CLI.
- [New Relic preflight outcome tagging](https://github.com/newrelic-experimental/preflight/pull/549): tags coding tasks by outcome type and model.

None of these combine launching, a local multi-subscription ledger, deterministic evals and measured prompt improvement.
