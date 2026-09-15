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
boxr --harness <h> --model <m> --effort <e> --account <profile> [--kind <k>] --p "<prompt>"
```

Harness, model, effort and account are explicit, with defaults from config.

### Headless (default)

The harness runs non-interactively and boxr reads its structured event stream.
By default the call blocks and prints a short TOON result: status, session id, trimmed final message, tokens, cost, duration, and next-step `help[]` lines.
`--detach` returns the session id immediately and the session keeps running in the background under boxr.
Detached sessions are managed with `boxr ps`, `boxr wait <id>` (optional timeout), `boxr status <id>`, `boxr tail <id>` and `boxr stop <id>`.
`boxr resume <id> "<prompt>"` continues a session.
If boxr crashes, the harness process is killed rather than orphaned, and the session is marked interrupted.

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
   Our own fields (profile, subscription, effort, kind) go in ATIF `extra`.
   `boxr export --atif <id>` wraps the lines into a standard ATIF document.
3. Summary: one line per session with harness, model, effort, profile, subscription, start, end, tokens, cost, status, kind and outcomes.
   All analytics query this layer.

Storage is JSONL on disk.
TOON is only the output shape when an agent reads through the CLI.

### Live capture

The main path is tailing the harness's transcript file live from launch to exit.
Hooks are the backup, for a harness that does not flush while running or for events the transcript does not record, such as waiting on the user.
Hooks are installed inside boxr's own profile directories, never in the user's normal harness setup.
Importing old transcripts uses the same parsers.

### Secrets

The raw layer is stored verbatim with owner-only permissions (0600 files in a 0700 directory) and is never read by any model.
The normalized and summary layers pass through redaction on write: known secret patterns (API key prefixes, JWTs, private key blocks, `KEY=value` env lines) plus user-listed values from config, replaced with `[REDACTED:<kind>]`.
Anything sent to a model (classification, eval mining, the prompt skill) reads only redacted layers.
The promise is "redacted where recognized", never "safe to share".

## Kinds

Fixed core: `build`, `fix`, `research`, `plan`, `review`, `chore`, `docs`.
Custom kinds are added in config, each with a one-line description the classifier can use.
One kind per session.

The caller declares a kind with `--kind`, recorded as `declared`.
Without one, a post-session pass with a cheap model assigns a kind, recorded as `inferred` with a confidence score.
Heuristics (no file edits, docs-only changes) feed hints into that pass and are not a separate system.

## Outcomes

Four separate fields, never blended into one score:

- Exit facts: exit code, error, limit hit, interruption. Automatic.
- Caller verdict: `boxr outcome <id> success|partial|failed --note "..."`. Optional and the strongest signal.
- Git evidence: commits made, files changed, and later whether those commits survived or were reverted. Automatic.
- Inferred judgment: the post-session pass judges whether the task was finished. Labeled inferred.

## Cost

Three measures, each labeled:

- Raw tokens: input, output, cached, reasoning. Ground truth.
- API-equivalent cost: tokens times list price from a config price table, in USD or EUR (configurable). The common currency for comparing models.
- Quota share: the percentage of a subscription window consumed, from quota readings before and after, split by token share when sessions overlap. Always marked estimated.

## Stats

`boxr stats` is the only source of numbers, for example `boxr stats --by model,kind --since 7d`.
Any visual view (a Lavish report, a later dashboard) is built on its output rather than querying the ledger itself.
Queries run on DuckDB reading the JSONL directly.
A cached DuckDB or Parquet file is added only when a measured need shows up.

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
It ships in this repo and installs with `boxr -i skills [--harness claude|codex|pi|all]`.

## Skills

Bundled skills live under `skills/<name>/` in this repo and are embedded in the binary at build time.
`boxr -i skills [--harness claude|codex|pi|all]` writes each bundled skill into that harness's user-level skill directory, and omitting `--harness` covers every supported harness.
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

## Milestones

1. Launch and ledger: headless and interactive, isolated profiles, subscriptions, live tailing into raw, ATIF JSONL and summary layers, redaction.
   Claude Code, Codex and pi.
   Done means every session launched through boxr is recorded live.
2. Stats: `--kind`, `boxr outcome`, the inferred kind and outcome pass, `boxr stats` on DuckDB, and import of existing transcripts.
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
