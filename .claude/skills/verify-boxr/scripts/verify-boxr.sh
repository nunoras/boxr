#!/bin/sh
set -eu

feature="${1:-headless-launch}"
skill_dir="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$skill_dir/../../.." && pwd)"

harness=claude
model=haiku
effort=low
budget=300
prompt="Reply with the single word ok and nothing else."
resume_prompt="Reply with the single word yes and nothing else."
declared_kind=describe
profile_name=verify
evidence_root="$HOME/.boxr-verify"

step="doctor"
say() { printf '%s\n' "$*"; }
fail() { code="$1"; shift; say "verify: failed" >&2; say "  step: $step" >&2; say "  why: $*" >&2; exit "$code"; }
die() { fail 1 "$@"; }
guard() {
  say "verify: failed"
  say "  step: guard"
  say "  why: $*"
  exit 2
}

field() { sed -n "s/^  $2: //p" "$1" | head -n 1; }
has_line() { grep -q "$2" "$1"; }

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -c1-64
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -c1-64
  else
    printf 'unavailable'
  fi
}

[ -z "${BOXR_HOME:-}" ] || guard "BOXR_HOME is already set to $BOXR_HOME; run this outside a boxr home"
[ -f "$repo_root/Cargo.toml" ] || guard "no boxr checkout found above $skill_dir"

config_json=""
sessions_spent=1
case "$feature" in
  headless-launch | outcomes | detached) ;;
  models-and-list)
    harness=pi
    ;;
  session-cost)
    config_json='{"currency":"USD","prices":{"haiku":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":60.0}}}'
    ;;
  resume) sessions_spent=2 ;;
  account-profiles) sessions_spent=0 ;;
  *) guard "unknown feature '$feature'; see $skill_dir/features/README.md" ;;
esac

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
run_dir="$evidence_root/runs/$stamp-$feature"
throwaway="$(mktemp -d "${TMPDIR:-/tmp}/boxr-verify.XXXXXX")"
boxr_home="$throwaway/home"
work="$throwaway/work"
meta="$run_dir/meta.txt"
active_pid=""
supervisor_pid=""
use_setsid=no
if command -v setsid >/dev/null 2>&1; then
  use_setsid=yes
fi

note() {
  if [ -f "$meta" ]; then
    printf '%s: %s\n' "$1" "$2" >>"$meta"
  fi
}

signal_group() {
  if [ "$use_setsid" = yes ]; then
    kill -"$1" -"$2" 2>/dev/null || kill -"$1" "$2" 2>/dev/null || true
  else
    kill -"$1" "$2" 2>/dev/null || true
  fi
}

stop_pid() {
  signal_group TERM "$1"
  sleep 2
  signal_group KILL "$1"
}

stop_supervisor() {
  if kill -0 "$supervisor_pid" 2>/dev/null; then
    note supervisorKilledByCleanup "$supervisor_pid"
    kill -TERM -"$supervisor_pid" 2>/dev/null || kill -TERM "$supervisor_pid" 2>/dev/null || true
    sleep 2
    kill -KILL -"$supervisor_pid" 2>/dev/null || kill -KILL "$supervisor_pid" 2>/dev/null || true
  fi
}

remove_throwaway() {
  cd "$repo_root"
  if [ -n "$active_pid" ]; then
    stop_pid "$active_pid"
    active_pid=""
  fi
  if [ -n "$supervisor_pid" ]; then
    stop_supervisor
    supervisor_pid=""
  fi
  [ -e "$throwaway" ] || return 0
  case "$throwaway" in
    */boxr-verify.*) rm -rf "$throwaway" || true ;;
    *) say "verify: refusing to remove the unexpected path $throwaway" >&2 ;;
  esac
  if [ -e "$throwaway" ]; then
    note throwawayRemoved no
  else
    note throwawayRemoved yes
  fi
}

cleanup() {
  code=$?
  remove_throwaway
  exit "$code"
}
trap cleanup EXIT
trap 'fail 130 "interrupted by SIGINT"' INT
trap 'fail 143 "terminated by SIGTERM"' TERM

bounded_exit=0
bounded_timed_out=no
bounded() {
  label="$1"
  shift
  if [ "$use_setsid" = yes ]; then
    setsid "$@" >"$run_dir/$label.txt" 2>"$run_dir/$label.stderr.txt" &
  else
    "$@" >"$run_dir/$label.txt" 2>"$run_dir/$label.stderr.txt" &
  fi
  active_pid=$!
  note "${label}Pid" "$active_pid"
  waited=0
  bounded_timed_out=no
  while kill -0 "$active_pid" 2>/dev/null; do
    if [ "$waited" -ge "$budget" ]; then
      bounded_timed_out=yes
      stop_pid "$active_pid"
      break
    fi
    sleep 1
    waited=$((waited + 1))
  done
  bounded_exit=0
  wait "$active_pid" || bounded_exit=$?
  active_pid=""
  note "${label}Exit" "$bounded_exit"
  note "${label}TimedOut" "$bounded_timed_out"
  [ "$bounded_timed_out" = no ] || die "boxr $label passed ${budget}s and was killed; read $run_dir/$label.txt"
}

boxr_exit=0
run_boxr() {
  label="$1"
  shift
  boxr_exit=0
  "$boxr_bin" "$@" >"$run_dir/$label.txt" 2>"$run_dir/$label.stderr.txt" || boxr_exit=$?
  note "${label}Exit" "$boxr_exit"
}

run_boxr_ok() {
  label="$1"
  run_boxr "$@"
  [ "$boxr_exit" -eq 0 ] || die "boxr $label exited $boxr_exit; read $run_dir/$label.txt and $run_dir/$label.stderr.txt"
}

expect() {
  has_line "$1" "$2" || die "$3; read $1"
}

keep_session() {
  [ -d "$boxr_home/sessions/$1" ] || die "boxr created no session $1 under $boxr_home"
  mkdir -p "$run_dir/sessions"
  cp -R "$boxr_home/sessions/$1" "$run_dir/sessions/$1"
}

transcript_source() {
  if [ -n "$1" ]; then
    find "$config_dir/projects" -name "$1.jsonl" -print 2>/dev/null | head -n 1
  fi
}

assert_recorded() {
  label="$1"
  id="$2"
  [ -n "$id" ] || die "the $label result carries no session id; read $run_dir/$label.txt"
  keep_session "$id"
  raw="$run_dir/sessions/$id/raw"
  [ -s "$raw/stream.jsonl" ] || die "the stream layer of $id is missing or empty"
  [ -s "$raw/transcript.jsonl" ] || die "the raw transcript of $id is missing or empty"
  expect "$run_dir/$label.txt" '^  ledger: recorded$' "the $label ledger was not recorded"
  expect "$run_dir/$label.txt" '^  status: ok$' "the $label result does not report status ok"
  [ -n "$(field "$run_dir/$label.txt" harnessSessionId)" ] || die "the $label result carries no harness session id"
}

launch_blocking() {
  label="$1"
  shift
  step="drive"
  bounded "$label" "$boxr_bin" --harness "$harness" --model "$model" --effort "$effort" "$@" -- "$prompt"
}

mkdir -p "$run_dir" "$boxr_home" "$work"
if [ -n "$config_json" ]; then
  printf '%s\n' "$config_json" >"$boxr_home/config.json"
fi
say "verify: feature $feature"
say "verify: evidence $run_dir"

say "verify: building the release binary"
if ! (cd "$repo_root" && cargo build --release) >"$run_dir/build.log" 2>&1; then
  die "cargo build --release failed; read $run_dir/build.log"
fi

boxr_bin="$repo_root/target/release/boxr"
[ -x "$boxr_bin" ] || boxr_bin="$boxr_bin.exe"
[ -x "$boxr_bin" ] || die "no release binary under $repo_root/target/release"

boxr_version="$("$boxr_bin" --version)"
git_head="$(cd "$repo_root" && git rev-parse HEAD)"
git_dirty="$(cd "$repo_root" && git status --porcelain | wc -l | tr -d ' ')"

if [ "$feature" = models-and-list ]; then
  step="models"
  BOXR_HOME="$boxr_home"
  export BOXR_HOME
  harness_path="$(command -v pi || true)"
  [ -n "$harness_path" ] || die "pi is not on PATH"
  {
    printf 'feature: %s\n' "$feature"
    printf 'runId: %s-%s\n' "$stamp" "$feature"
    printf 'date: %s\n' "$stamp"
    printf 'boxrVersion: %s\n' "$boxr_version"
    printf 'boxrBinary: %s\n' "$boxr_bin"
    printf 'boxrBinarySha256: %s\n' "$(sha256 "$boxr_bin")"
    printf 'gitHead: %s\n' "$git_head"
    printf 'gitDirtyFiles: %s\n' "$git_dirty"
    printf 'harness: pi\n'
    printf 'harnessPath: %s\n' "$harness_path"
    printf 'throwaway: %s\n' "$throwaway"
  } >"$meta"

  say "verify: reading the pi model catalog (no model is launched)"
  "$boxr_bin" models --harness pi >"$run_dir/models.txt" 2>"$run_dir/stderr.txt" \
    || die "boxr models --harness pi failed; read $run_dir/models.txt"
  grep -q '^models\[' "$run_dir/models.txt" || die "boxr models printed no models table"
  grep -q '^  [a-z0-9-]*,[a-z0-9.-]*$' "$run_dir/models.txt" || die "boxr models printed no provider,model rows"

  "$boxr_bin" models --harness claude >"$run_dir/unsupported.txt" 2>&1 && \
    die "boxr models --harness claude should be a usage error"
  grep -q 'does not expose a model catalog' "$run_dir/unsupported.txt" \
    || die "the unsupported-harness error is unclear; read $run_dir/unsupported.txt"

  step="list"
  "$boxr_bin" list >"$run_dir/list.txt" 2>>"$run_dir/stderr.txt" \
    || die "boxr list failed; read $run_dir/list.txt"
  grep -q '^list\[0\]{id,state,harness,model,status,start,durationMs,kind,verdict}:' "$run_dir/list.txt" \
    || die "boxr list has the wrong shape; read $run_dir/list.txt"

  step="cleanup"
  remove_throwaway
  [ ! -e "$throwaway" ] || die "the throwaway home still exists at $throwaway"
  say "verify:"
  say "  feature: $feature"
  say "  result: ok"
  say "  evidence: $run_dir"
  say "help[2]:"
  say "  Read the model table at $run_dir/models.txt"
  say "  Read $run_dir/meta.txt for the doctor facts behind this run"
  exit 0
fi

harness_path="$(command -v "$harness" || true)"
[ -n "$harness_path" ] || die "harness '$harness' is not on PATH"
harness_version="$("$harness" --version 2>&1 | head -n 1)"

config_dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
profile_source=ambient
if [ -n "${CLAUDE_CONFIG_DIR:-}" ]; then
  profile_source=CLAUDE_CONFIG_DIR
fi
env_login=no
if [ -n "${ANTHROPIC_API_KEY:-}" ] || [ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ]; then
  env_login=yes
fi
logged_in="$env_login"
if [ -f "$config_dir/.credentials.json" ]; then
  logged_in=yes
fi
if [ "$(uname -s)" = Darwin ] && security find-generic-password -s 'Claude Code-credentials' >/dev/null 2>&1; then
  logged_in=yes
fi
if [ "$feature" = account-profiles ]; then
  [ "$env_login" = no ] || die "ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN is set, so the empty profile would still log in and spend quota; unset them"
else
  [ "$logged_in" = yes ] || die "no login in $config_dir; run '$harness' once to log in, or set ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN"
fi

{
  printf 'feature: %s\n' "$feature"
  printf 'runId: %s-%s\n' "$stamp" "$feature"
  printf 'date: %s\n' "$stamp"
  printf 'boxrVersion: %s\n' "$boxr_version"
  printf 'boxrBinary: %s\n' "$boxr_bin"
  printf 'boxrBinarySha256: %s\n' "$(sha256 "$boxr_bin")"
  printf 'gitHead: %s\n' "$git_head"
  printf 'gitDirtyFiles: %s\n' "$git_dirty"
  printf 'harness: %s\n' "$harness"
  printf 'harnessPath: %s\n' "$harness_path"
  printf 'harnessVersion: %s\n' "$harness_version"
  printf 'harnessConfigDir: %s\n' "$config_dir"
  printf 'harnessProfileSource: %s\n' "$profile_source"
  printf 'model: %s\n' "$model"
  printf 'effort: %s\n' "$effort"
  printf 'prompt: %s\n' "$prompt"
  printf 'paidSessions: %s\n' "$sessions_spent"
  printf 'configJson: %s\n' "${config_json:-none}"
  printf 'throwaway: %s\n' "$throwaway"
} >"$meta"

if [ "$feature" = outcomes ]; then
  git -C "$work" init -q || die "git init failed in $work"
  git -C "$work" -c user.name=boxr-verify -c user.email=verify@boxr.invalid -c commit.gpgsign=false \
    commit -q --allow-empty -m "verify baseline" || die "the baseline commit failed in $work"
  note gitBase "$(git -C "$work" rev-parse HEAD)"
fi
cd "$work"
BOXR_HOME="$boxr_home"
export BOXR_HOME

session_id=""

drive_launch() {
  say "verify: driving a real $harness session on $model (this spends quota)"
  launch_blocking launch
  session_id="$(field "$run_dir/launch.txt" id)"
  harness_session_id="$(field "$run_dir/launch.txt" harnessSessionId)"
  note sessionId "$session_id"
  note harnessSessionId "$harness_session_id"
  note harnessTranscriptSource "$(transcript_source "$harness_session_id")"
  step="evidence"
  [ "$bounded_exit" -eq 0 ] || { [ -z "$session_id" ] || keep_session "$session_id"; die "boxr exited $bounded_exit; read $run_dir/launch.txt and $run_dir/launch.stderr.txt"; }
  assert_recorded launch "$session_id"
}

drive_outcomes() {
  step="outcome"
  run_boxr_ok outcome outcome "$session_id" success --note verified
  expect "$run_dir/outcome.txt" '^outcome:$' "the outcome result has no outcome: section"
  expect "$run_dir/outcome.txt" '^  verdict: success$' "the outcome result does not report the recorded verdict"
  expect "$run_dir/outcome.txt" '^  note: verified$' "the outcome result does not report the recorded note"

  run_boxr_ok show show "$session_id"
  expect "$run_dir/show.txt" '^  verdict: success$' "boxr show does not fold the recorded verdict"
  expect "$run_dir/show.txt" '^  verdictNote: verified$' "boxr show does not fold the recorded note"

  run_boxr_ok stats stats --by verdict --since 7d
  expect "$run_dir/stats.txt" '^  success,USD,1,' "boxr stats does not group the session under its recorded verdict"

  run_boxr_ok reverts outcome --check-reverted "$session_id"
  expect "$run_dir/reverts.txt" '^reverts:$' "the revert check has no reverts: section"
  expect "$run_dir/reverts.txt" '^  commits: 0$' "the revert check did not report the recorded commit count"

  summary_records="$(wc -l <"$boxr_home/summary.jsonl" | tr -d ' ')"
  [ "$summary_records" -ge 3 ] || die "summary.jsonl holds $summary_records records; expected the full summary plus the verdict and revert updates"
}

drive_session_cost() {
  step="cost"
  launch_toon="$run_dir/launch.txt"
  priced_cost="$(field "$launch_toon" apiEquivalentCost)"
  priced_currency="$(field "$launch_toon" currency)"
  [ "$priced_currency" = USD ] || die "the launch records currency '$priced_currency' instead of USD"
  awk -v value="$priced_cost" 'BEGIN { if (value + 0 <= 0) exit 1; exit 0 }' ||
    die "the launch records apiEquivalentCost '$priced_cost' instead of a positive amount"
  if has_line "$launch_toon" '^  costError: '; then
    die "the launch recorded a cost error: $(field "$launch_toon" costError)"
  fi

  summary_json="$(tail -n 1 "$boxr_home/summary.jsonl")"
  json_field() { printf '%s\n' "$summary_json" | tr ',' '\n' | sed -n "s/^\"$1\":\(.*\)$/\1/p" | tr -d '"'; }

  arithmetic="$(awk \
    -v prompt="$(json_field promptTokens)" \
    -v completion="$(json_field completionTokens)" \
    -v cached="$(json_field cachedTokens)" \
    -v reasoning="$(json_field reasoningTokens)" \
    -v recorded="$(json_field apiEquivalentCost)" 'BEGIN {
      if (cached > prompt) prompt = cached
      if (reasoning > completion) completion = reasoning
      expected = ((prompt - cached) * 2.0 + cached * 0.3 + (completion - reasoning) * 6.0 + reasoning * 60.0) / 1000000.0
      if (recorded <= 0) { printf "the summary records %s", recorded; exit 1 }
      diff = recorded - expected
      if (diff < 0) diff = -diff
      if (diff > expected * 1e-9 + 1e-12) { printf "expected %s, recorded %s", expected, recorded; exit 1 }
    }')" || die "the summary cost does not match the price table arithmetic: $arithmetic"
  [ "$(json_field currency)" = USD ] || die "the summary records currency $(json_field currency) instead of USD"

  run_boxr_ok show show "$session_id"
  shown_cost="$(field "$run_dir/show.txt" apiEquivalentCost)"
  [ "$shown_cost" = "$priced_cost" ] || die "boxr show reads $shown_cost but the launch recorded $priced_cost"
  [ "$(field "$run_dir/show.txt" currency)" = USD ] || die "boxr show does not read the recorded currency USD"

  run_boxr_ok stats stats --by model --since 1d
  stats_row="$(sed -n 's/^  //p' "$run_dir/stats.txt" | grep '^haiku,USD,' | head -n 1)"
  [ -n "$stats_row" ] || die "stats has no haiku,USD group; read $run_dir/stats.txt"
  [ "$(printf '%s\n' "$stats_row" | cut -d, -f6)" = "$priced_cost" ] ||
    die "stats totals a different cost than the launch recorded; read $run_dir/stats.txt"
  [ "$(printf '%s\n' "$stats_row" | cut -d, -f7)" = 0 ] ||
    die "stats counts a priced session as unpriced; read $run_dir/stats.txt"
}

drive_detached() {
  say "verify: driving a real detached $harness session on $model (this spends quota)"
  step="detach"
  run_boxr_ok detach --harness "$harness" --model "$model" --effort "$effort" --kind "$declared_kind" --detach -- "$prompt"
  session_id="$(field "$run_dir/detach.txt" id)"
  supervisor_pid="$(field "$run_dir/detach.txt" pid)"
  note sessionId "$session_id"
  note supervisorPid "$supervisor_pid"
  [ -n "$session_id" ] || die "the detach result carries no session id"
  [ -n "$supervisor_pid" ] || die "the detach result carries no supervisor pid"
  expect "$run_dir/detach.txt" '^  status: running$' "the detach result does not report status running"

  step="ps"
  run_boxr_ok ps ps
  if ! has_line "$run_dir/ps.txt" "^  $session_id,running,$harness,$model$"; then
    run_boxr status-late status "$session_id"
    die "boxr ps does not list $session_id as running; if it already finished the running path is unproven this run"
  fi
  run_boxr_ok status-running status "$session_id"
  expect "$run_dir/status-running.txt" '^  state: running$' "boxr status does not report the live session as running"
  expect "$run_dir/status-running.txt" "^  pid: $supervisor_pid$" "boxr status does not report the supervisor pid detach printed"

  step="tail"
  bounded tail "$boxr_bin" tail "$session_id"
  [ "$bounded_exit" -eq 0 ] || die "boxr tail exited $bounded_exit"
  [ -s "$run_dir/tail.txt" ] || die "boxr tail streamed nothing"

  step="wait"
  bounded wait "$boxr_bin" wait --timeout "$budget" "$session_id"
  [ "$bounded_exit" -eq 0 ] || die "boxr wait exited $bounded_exit"
  expect "$run_dir/wait.txt" '^  status: ok$' "boxr wait does not report status ok"
  expect "$run_dir/wait.txt" '^  state: finished$' "boxr wait does not report state finished"
  harness_session_id="$(field "$run_dir/wait.txt" harnessSessionId)"
  note harnessSessionId "$harness_session_id"
  note harnessTranscriptSource "$(transcript_source "$harness_session_id")"

  waited=0
  while kill -0 "$supervisor_pid" 2>/dev/null && [ "$waited" -lt 10 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if kill -0 "$supervisor_pid" 2>/dev/null; then
    die "the supervisor $supervisor_pid is still alive after the session finished"
  fi
  note supervisorExited yes
  supervisor_pid=""

  step="evidence"
  assert_recorded wait "$session_id"
  tail_lines="$(wc -l <"$run_dir/tail.txt" | tr -d ' ')"
  normalized_lines="$(wc -l <"$run_dir/sessions/$session_id/normalized.jsonl" | tr -d ' ')"
  [ "$tail_lines" = "$normalized_lines" ] || die "boxr tail printed $tail_lines lines but normalized.jsonl holds $normalized_lines"
  cmp -s "$run_dir/tail.txt" "$run_dir/sessions/$session_id/normalized.jsonl" || die "boxr tail output differs from normalized.jsonl"
  for record in launch.json supervisor.json report.json; do
    [ -s "$run_dir/sessions/$session_id/$record" ] || die "the session directory has no $record"
  done

  step="after"
  run_boxr_ok status-finished status "$session_id"
  expect "$run_dir/status-finished.txt" '^  state: finished$' "boxr status does not report the finished session as finished"
  run_boxr_ok ps-after ps
  expect "$run_dir/ps-after.txt" '^  running: 0$' "boxr ps still lists a running session"
  run_boxr_ok stop stop "$session_id"
  expect "$run_dir/stop.txt" '^  action: already-finished$' "boxr stop on a finished session does not report already-finished"
  expect "$run_dir/stop.txt" '^  status: ok$' "boxr stop changed the status of a finished session"

  run_boxr_ok show show "$session_id"
  expect "$run_dir/show.txt" '^  status: ok$' "boxr show does not report status ok"
  expect "$run_dir/show.txt" '^  mode: headless$' "boxr show does not report mode headless"
  expect "$run_dir/show.txt" "^  kind: $declared_kind$" "boxr show does not report the declared kind"
  expect "$run_dir/show.txt" '^  kindSource: declared$' "boxr show does not report kindSource declared"

  run_boxr_ok export export --atif "$session_id"
  expect "$run_dir/export.txt" '^  schemaVersion: ATIF-v1.8$' "boxr export does not report schema ATIF-v1.8"
  trajectory="$(field "$run_dir/export.txt" path)"
  [ -s "$trajectory" ] || die "the exported trajectory is missing at $trajectory"
  cp "$trajectory" "$run_dir/trajectory.atif.json"
  expect "$run_dir/trajectory.atif.json" '"ATIF-v1.8"' "the trajectory does not carry schema ATIF-v1.8"
  expect "$run_dir/trajectory.atif.json" "\"$session_id\"" "the trajectory does not carry the session id"
}

drive_resume() {
  step="resume"
  say "verify: resuming $session_id (this spends quota again)"
  bounded resume "$boxr_bin" resume "$session_id" "$resume_prompt"
  [ "$bounded_exit" -eq 0 ] || die "boxr resume exited $bounded_exit"
  continuation="$(field "$run_dir/resume.txt" id)"
  note continuationId "$continuation"
  [ -n "$continuation" ] && [ "$continuation" != "$session_id" ] || die "boxr resume did not record a new session id"
  expect "$run_dir/resume.txt" "^  resumedFrom: $session_id$" "the resume result does not link back to $session_id"
  [ "$(field "$run_dir/resume.txt" harnessSessionId)" = "$(field "$run_dir/launch.txt" harnessSessionId)" ] ||
    die "the continuation reports a different harness session id than the original"
  assert_recorded resume "$continuation"
  resumed_steps="$(field "$run_dir/resume.txt" steps)"
  [ "${resumed_steps:-0}" -ge 1 ] || die "the continuation normalized no steps"

  run_boxr_ok show show "$continuation"
  expect "$run_dir/show.txt" '^  mode: resume$' "boxr show does not report mode resume"
  expect "$run_dir/show.txt" "^  resumedFrom: $session_id$" "boxr show does not report resumedFrom"
  run_boxr_ok show-origin show "$session_id"
  expect "$run_dir/show-origin.txt" '^  mode: headless$' "the original session changed mode after resume"
}

drive_account_profiles() {
  step="accounts"
  profile_dir="$boxr_home/accounts/$harness/$profile_name"
  run_boxr_ok accounts-empty account list
  expect "$run_dir/accounts-empty.txt" '^accounts\[0\]' "boxr account list is not empty in a fresh home"

  run_boxr missing-profile --harness "$harness" --model "$model" --account "$profile_name" -- "$prompt"
  [ "$boxr_exit" -eq 2 ] || die "a launch with an unknown profile exited $boxr_exit instead of 2"
  [ ! -d "$boxr_home/sessions" ] || [ -z "$(ls -A "$boxr_home/sessions")" ] || die "a launch with an unknown profile still created a session"

  mkdir -p "$profile_dir"
  note scaffoldProfile "$profile_dir"
  run_boxr_ok accounts-listed account list
  expect "$run_dir/accounts-listed.txt" "^  $harness,$profile_name," "boxr account list does not show the $profile_name profile"

  step="drive"
  bounded launch "$boxr_bin" --harness "$harness" --model "$model" --effort "$effort" --account "$profile_name" -- "$prompt"
  [ "$bounded_exit" -ne 0 ] || die "a launch on an empty profile succeeded, so it did not use the profile"
  session_id="$(field "$run_dir/launch.txt" id)"
  note sessionId "$session_id"
  [ -n "$session_id" ] || die "the profiled launch recorded no session; read $run_dir/launch.stderr.txt"
  keep_session "$session_id"
  expect "$run_dir/launch.txt" "^  account: $profile_name$" "the launch result does not name the profile"
  [ -n "$(ls -A "$profile_dir")" ] || die "the harness wrote nothing into the profile directory, so it did not run there"
  ls -A "$profile_dir" >"$run_dir/profile-contents.txt"
  run_boxr_ok show show "$session_id"
  expect "$run_dir/show.txt" "^  profile: $profile_name$" "boxr show does not record the profile"

  step="remove"
  run_boxr remove-unconfirmed account remove --harness "$harness" --name "$profile_name"
  [ "$boxr_exit" -eq 2 ] || die "account remove without --yes exited $boxr_exit instead of 2"
  [ -d "$profile_dir" ] || die "account remove without --yes deleted the profile"
  run_boxr_ok remove account remove --harness "$harness" --name "$profile_name" --yes
  expect "$run_dir/remove.txt" '^  status: removed$' "account remove does not report removed"
  [ ! -e "$profile_dir" ] || die "account remove --yes left $profile_dir behind"
}

case "$feature" in
  headless-launch | outcomes | session-cost | resume) drive_launch ;;
esac
case "$feature" in
  outcomes) drive_outcomes ;;
  session-cost) drive_session_cost ;;
  detached) drive_detached ;;
  resume) drive_resume ;;
  account-profiles) drive_account_profiles ;;
esac

if [ -f "$boxr_home/summary.jsonl" ]; then
  cp "$boxr_home/summary.jsonl" "$run_dir/summary.jsonl"
fi

step="cleanup"
remove_throwaway
[ ! -e "$throwaway" ] || die "the throwaway home still exists at $throwaway"
[ -f "$meta" ] && [ -d "$run_dir/sessions" ] || die "the evidence did not survive cleanup at $run_dir"

say "verify:"
say "  feature: $feature"
say "  result: ok"
say "  evidence: $run_dir"
say "  sessionId: $session_id"
say "  paidSessions: $sessions_spent"
say "  model: $model"
say "help[2]:"
say "  Read $run_dir/meta.txt for the doctor facts behind this run"
say "  Read $run_dir/sessions/ for the session directories the run recorded"
