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
evidence_root="$HOME/.boxr-verify"

step="doctor"
say() { printf '%s\n' "$*"; }
fail() { code="$1"; shift; say "verify: failed" >&2; say "  step: $step" >&2; say "  why: $*" >&2; exit "$code"; }
die() { fail 1 "$@"; }

field() { sed -n "s/^  $2: //p" "$1" | head -n 1; }

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -c1-64
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -c1-64
  else
    printf 'unavailable'
  fi
}

if [ -n "${BOXR_HOME:-}" ]; then
  say "verify: failed"
  say "  step: guard"
  say "  why: BOXR_HOME is already set to $BOXR_HOME; run this outside a boxr home"
  exit 2
fi

[ -f "$repo_root/Cargo.toml" ] || {
  say "verify: failed"
  say "  step: guard"
  say "  why: no boxr checkout found above $skill_dir"
  exit 2
}

case "$feature" in
  headless-launch | outcomes) ;;
  *)
    say "verify: failed"
    say "  step: guard"
    say "  why: unknown feature '$feature'; see $skill_dir/features/README.md"
    exit 2
    ;;
esac

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
run_dir="$evidence_root/runs/$stamp-$feature"
throwaway="$(mktemp -d "${TMPDIR:-/tmp}/boxr-verify.XXXXXX")"
boxr_home="$throwaway/home"
work="$throwaway/work"
meta="$run_dir/meta.txt"
boxr_pid=""
use_setsid=no
if command -v setsid >/dev/null 2>&1; then
  use_setsid=yes
fi

note() {
  if [ -f "$meta" ]; then
    printf '%s: %s\n' "$1" "$2" >>"$meta"
  fi
}

signal_boxr() {
  if [ "$use_setsid" = yes ]; then
    kill -"$1" -"$boxr_pid" 2>/dev/null || kill -"$1" "$boxr_pid" 2>/dev/null || true
  else
    kill -"$1" "$boxr_pid" 2>/dev/null || true
  fi
}

stop_boxr() {
  signal_boxr TERM
  sleep 2
  signal_boxr KILL
}

remove_throwaway() {
  cd "$repo_root"
  if [ -n "$boxr_pid" ]; then
    stop_boxr
    boxr_pid=""
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

mkdir -p "$run_dir" "$boxr_home" "$work"
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

harness_path="$(command -v "$harness" || true)"
[ -n "$harness_path" ] || die "harness '$harness' is not on PATH"
harness_version="$("$harness" --version 2>&1 | head -n 1)"

config_dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
profile_source=ambient
if [ -n "${CLAUDE_CONFIG_DIR:-}" ]; then
  profile_source=CLAUDE_CONFIG_DIR
fi
logged_in=no
if [ -f "$config_dir/.credentials.json" ]; then
  logged_in=yes
fi
if [ -n "${ANTHROPIC_API_KEY:-}" ] || [ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ]; then
  logged_in=yes
fi
if [ "$(uname -s)" = Darwin ] && security find-generic-password -s 'Claude Code-credentials' >/dev/null 2>&1; then
  logged_in=yes
fi
[ "$logged_in" = yes ] || die "no login in $config_dir; run '$harness' once to log in, or set ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN"

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
  printf 'throwaway: %s\n' "$throwaway"
} >"$meta"

say "verify: driving a real $harness session on $model (this spends quota)"
step="drive"
if [ "$feature" = outcomes ]; then
  git -C "$work" init -q || die "git init failed in $work"
  git -C "$work" -c user.name=boxr-verify -c user.email=verify@boxr.invalid -c commit.gpgsign=false \
    commit -q --allow-empty -m "verify baseline" || die "the baseline commit failed in $work"
  note gitBase "$(git -C "$work" rev-parse HEAD)"
fi
cd "$work"
BOXR_HOME="$boxr_home"
export BOXR_HOME

timed_out=no

launch() {
  set -- "$boxr_bin" --harness "$harness" --model "$model" --effort "$effort" -- "$prompt"
  if [ "$use_setsid" = yes ]; then
    setsid "$@" >"$run_dir/toon.txt" 2>"$run_dir/stderr.txt" &
  else
    "$@" >"$run_dir/toon.txt" 2>"$run_dir/stderr.txt" &
  fi
  boxr_pid=$!
}

launch
note boxrPid "$boxr_pid"
waited=0
while kill -0 "$boxr_pid" 2>/dev/null; do
  if [ "$waited" -ge "$budget" ]; then
    timed_out=yes
    break
  fi
  sleep 1
  waited=$((waited + 1))
done
if [ "$timed_out" = yes ]; then
  stop_boxr
fi
exit_code=0
wait "$boxr_pid" || exit_code=$?
boxr_pid=""
note exitCode "$exit_code"
note timedOut "$timed_out"

step="evidence"
session_id="$(field "$run_dir/toon.txt" id)"
harness_session_id="$(field "$run_dir/toon.txt" harnessSessionId)"
source_transcript=""
if [ -n "$harness_session_id" ]; then
  source_transcript="$(find "$config_dir/projects" -name "$harness_session_id.jsonl" -print 2>/dev/null | head -n 1)"
fi
note sessionId "$session_id"
note harnessSessionId "$harness_session_id"
note harnessTranscriptSource "${source_transcript:-not found}"

session_dir=""
for candidate in "$boxr_home"/sessions/*/; do
  if [ -d "$candidate" ]; then
    session_dir="$candidate"
  fi
done
[ -n "$session_dir" ] || die "boxr created no session under $boxr_home; read $run_dir/stderr.txt"
cp -R "$session_dir/raw" "$run_dir/raw"

if [ "$timed_out" = yes ]; then
  die "the session passed ${budget}s and was killed"
fi
[ "$exit_code" -eq 0 ] || die "boxr exited $exit_code; read $run_dir/toon.txt and $run_dir/stderr.txt"
[ -s "$run_dir/raw/stream.jsonl" ] || die "the stream layer is missing or empty"
[ -s "$run_dir/raw/transcript.jsonl" ] || die "the raw transcript is missing or empty"
grep -q '^  ledger: recorded$' "$run_dir/toon.txt" || die "the ledger was not recorded"
grep -q '^  status: ok$' "$run_dir/toon.txt" || die "the TOON result does not report status ok"
[ -n "$session_id" ] || die "the TOON result carries no session id"
[ -n "$harness_session_id" ] || die "the TOON result carries no harness session id"

if [ "$feature" = outcomes ]; then
  step="outcome"
  outcome_note=verified
  "$boxr_bin" outcome "$session_id" success --note "$outcome_note" >"$run_dir/outcome.txt" 2>>"$run_dir/stderr.txt" \
    || die "boxr outcome failed for $session_id; read $run_dir/outcome.txt"
  grep -q '^outcome:$' "$run_dir/outcome.txt" || die "the outcome result has no outcome: section"
  grep -q '^  verdict: success$' "$run_dir/outcome.txt" || die "the outcome result does not report the recorded verdict"
  grep -q "^  note: $outcome_note$" "$run_dir/outcome.txt" || die "the outcome result does not report the recorded note"

  "$boxr_bin" show "$session_id" >"$run_dir/show.txt" 2>>"$run_dir/stderr.txt" \
    || die "boxr show failed for $session_id; read $run_dir/show.txt"
  grep -q '^  verdict: success$' "$run_dir/show.txt" || die "boxr show does not fold the recorded verdict"
  grep -q "^  verdictNote: $outcome_note$" "$run_dir/show.txt" || die "boxr show does not fold the recorded note"

  "$boxr_bin" stats --by verdict --since 7d >"$run_dir/stats.txt" 2>>"$run_dir/stderr.txt" \
    || die "boxr stats failed; read $run_dir/stats.txt"
  grep -q '^  success,1,' "$run_dir/stats.txt" || die "boxr stats does not group the session under its recorded verdict"

  "$boxr_bin" outcome --check-reverted "$session_id" >"$run_dir/reverts.txt" 2>>"$run_dir/stderr.txt" \
    || die "the revert check failed for $session_id; read $run_dir/reverts.txt"
  grep -q '^reverts:$' "$run_dir/reverts.txt" || die "the revert check has no reverts: section"
  grep -q '^  commits: 0$' "$run_dir/reverts.txt" || die "the revert check did not report the recorded commit count"

  summary_records="$(wc -l <"$boxr_home/summary.jsonl" | tr -d ' ')"
  [ "$summary_records" -ge 3 ] || die "summary.jsonl holds $summary_records records; expected the full summary plus the verdict and revert updates"
fi

step="cleanup"
remove_throwaway
if [ -e "$throwaway" ]; then
  die "the throwaway home still exists at $throwaway"
fi

if [ ! -d "$run_dir/raw" ]; then
  die "the evidence did not survive cleanup at $run_dir"
fi

say "verify:"
say "  feature: $feature"
say "  result: ok"
say "  evidence: $run_dir"
say "  sessionId: $session_id"
say "  harnessSessionId: $harness_session_id"
say "  model: $model"
say "help[2]:"
say "  Read the raw transcript at $run_dir/raw/transcript.jsonl"
say "  Read $run_dir/meta.txt for the doctor facts behind this run"