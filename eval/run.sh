#!/usr/bin/env bash
# Skill evaluation runner for presto.
#
# Runs test cases against AI agent CLIs to measure how well SKILL.md
# teaches them when and how to use presto.
#
# Usage:
#   ./eval/run.sh                              # Run all cases with amp
#   ./eval/run.sh --agent amp                  # Explicit agent
#   ./eval/run.sh --agent claude               # Use Claude Code
#   ./eval/run.sh --category trigger-positive  # Filter by category
#   ./eval/run.sh --case llm-ask-gpt           # Run single case
#   ./eval/run.sh --skill eval/variants/v2.md   # A/B test a SKILL.md variant
#   ./eval/run.sh --parallel 8                 # Run 8 cases concurrently
#   ./eval/run.sh --dry-run                    # Show what would run
#
# Environment:
#   EVAL_TIMEOUT  - Per-case timeout in seconds (default: 180)
#   EVAL_CASES    - Path to cases file (default: eval/cases/cases.json)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Defaults
AGENT="amp"
CATEGORY=""
CASE_FILTER=""
DRY_RUN=false
TIMEOUT="${EVAL_TIMEOUT:-180}"
CASES_FILE="${EVAL_CASES:-${SCRIPT_DIR}/cases/cases.json}"
SKILL_FILE=""
PARALLEL=5

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --agent)     AGENT="$2"; shift 2 ;;
    --category)  CATEGORY="$2"; shift 2 ;;
    --case)      CASE_FILTER="$2"; shift 2 ;;
    --dry-run)   DRY_RUN=true; shift ;;
    --timeout)   TIMEOUT="$2"; shift 2 ;;
    --cases)     CASES_FILE="$2"; shift 2 ;;
    --skill)     SKILL_FILE="$2"; shift 2 ;;
    --parallel|-j) PARALLEL="$2"; shift 2 ;;
    --sequential) PARALLEL=1; shift ;;
    -h|--help)
      echo "Usage: $0 [--agent amp|claude] [--category CAT] [--case ID] [--skill PATH] [--parallel N] [--sequential] [--dry-run] [--timeout SECS]"
      exit 0
      ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# Validate
if ! command -v "$AGENT" &>/dev/null; then
  echo "Error: agent CLI '$AGENT' not found in PATH"
  exit 1
fi

if ! command -v jq &>/dev/null; then
  echo "Error: jq is required"
  exit 1
fi

if [ ! -f "$CASES_FILE" ]; then
  echo "Error: cases file not found: $CASES_FILE"
  exit 1
fi

# Check installed presto matches the repo version
if command -v presto &>/dev/null; then
  INSTALLED_HASH=$(md5 -q "$(which presto)" 2>/dev/null || md5sum "$(which presto)" 2>/dev/null | awk '{print $1}')
  BUILD_HASH=$(md5 -q "${REPO_DIR}/target/release/presto" 2>/dev/null || true)
  if [ -n "$BUILD_HASH" ] && [ "$INSTALLED_HASH" != "$BUILD_HASH" ]; then
    echo "Warning: installed presto ($(which presto)) differs from repo build"
    echo "  Run 'make install' to update"
    echo ""
  fi
fi

# Resolve timeout command: timeout (Linux), gtimeout (brew coreutils), or builtin fallback
if command -v timeout &>/dev/null; then
  TIMEOUT_CMD="timeout"
elif command -v gtimeout &>/dev/null; then
  TIMEOUT_CMD="gtimeout"
else
  TIMEOUT_CMD="builtin"
fi

# Create run directory
SKILL_LABEL=""
if [ -n "$SKILL_FILE" ]; then
  SKILL_LABEL=$(basename "$SKILL_FILE" .md | tr ' ' '-')
  RUN_ID="$(date +%Y%m%d-%H%M%S)-${AGENT}-${SKILL_LABEL}"
else
  RUN_ID="$(date +%Y%m%d-%H%M%S)-${AGENT}"
fi
RUN_DIR="${SCRIPT_DIR}/runs/${RUN_ID}"
mkdir -p "$RUN_DIR"

# A/B testing: swap SKILL.md if --skill is provided
SKILL_BACKUP=""
SKILL_TARGETS=()
if [ -n "$SKILL_FILE" ]; then
  if [ ! -f "$SKILL_FILE" ]; then
    echo "Error: skill file not found: $SKILL_FILE"
    exit 1
  fi
  SKILL_TARGETS=("${REPO_DIR}/.ai/skills/presto/SKILL.md")
  if [ -f "${HOME}/.claude/skills/presto/SKILL.md" ]; then
    SKILL_TARGETS+=("${HOME}/.claude/skills/presto/SKILL.md")
  fi
  SKILL_BACKUP="${RUN_DIR}/.skill-backup"
  mkdir -p "$SKILL_BACKUP"
  for target in "${SKILL_TARGETS[@]}"; do
    backup_name=$(echo "$target" | tr '/' '_')
    cp "$target" "${SKILL_BACKUP}/${backup_name}"
    cp "$SKILL_FILE" "$target"
  done
  cp "$SKILL_FILE" "${RUN_DIR}/SKILL.md"
  trap 'for target in "${SKILL_TARGETS[@]}"; do backup_name=$(echo "$target" | tr "/" "_"); cp "${SKILL_BACKUP}/${backup_name}" "$target" 2>/dev/null || true; done' EXIT
fi

SUITE_START=$(date +%s)

echo "=== Presto Skill Eval ==="
echo "Agent:    $AGENT"
echo "Cases:    $CASES_FILE"
if [ -n "$SKILL_FILE" ]; then
  echo "Skill:    $SKILL_FILE"
fi
echo "Parallel: $PARALLEL"
echo "Run dir:  $RUN_DIR"
echo "Timeout:  ${TIMEOUT}s per case"
echo ""

# Build case list (apply filters)
CASE_JQ_FILTER='.cases[]'
if [ -n "$CATEGORY" ]; then
  CASE_JQ_FILTER="${CASE_JQ_FILTER} | select(.category == \"${CATEGORY}\")"
fi
if [ -n "$CASE_FILTER" ]; then
  CASE_JQ_FILTER="${CASE_JQ_FILTER} | select(.id == \"${CASE_FILTER}\")"
fi

CASE_IDS=$(jq -r "$CASE_JQ_FILTER | .id" "$CASES_FILE")
TOTAL=$(echo "$CASE_IDS" | grep -c . || echo 0)

if [ "$TOTAL" -eq 0 ]; then
  echo "No cases matched filters."
  exit 0
fi

echo "Running $TOTAL cases..."
echo ""

if [ "$DRY_RUN" = "true" ]; then
  echo "--- DRY RUN ---"
  for case_id in $CASE_IDS; do
    CASE_JSON=$(jq -c ".cases[] | select(.id == \"${case_id}\")" "$CASES_FILE")
    PROMPT=$(echo "$CASE_JSON" | jq -r '.prompt')
    CATEGORY_VAL=$(echo "$CASE_JSON" | jq -r '.category')
    echo "  [$CATEGORY_VAL] $case_id: $PROMPT"
  done
  exit 0
fi

# Run a command with a timeout. Uses system `timeout`/`gtimeout` if available,
# otherwise a perl-based fallback (available on macOS).
# Returns 124 on timeout (matching GNU coreutils convention).
run_with_timeout() {
  local secs="$1"
  shift

  if [ "$TIMEOUT_CMD" != "builtin" ]; then
    $TIMEOUT_CMD "${secs}s" "$@"
    return $?
  fi

  perl -e '
    $SIG{ALRM} = sub { kill("TERM", $pid); exit(124) };
    alarm(shift @ARGV);
    $pid = fork();
    if ($pid == 0) { exec(@ARGV) or exit(127); }
    waitpid($pid, 0);
    exit($? >> 8);
  ' "$secs" "$@"
}

run_agent() {
  local prompt="$1"
  local outfile="$2"
  local errfile="${outfile%.jsonl}.stderr"

  # Run in a temp directory to prevent side effects (file creation) from
  # polluting the repo. The global skill at ~/.claude/skills/ is still found.
  local workdir
  workdir=$(mktemp -d)
  local rc=0

  case "$AGENT" in
    amp)
      (cd "$workdir" && run_with_timeout "$TIMEOUT" amp --stream-json -x "$prompt" < /dev/null > "$outfile" 2>"$errfile") || rc=$?
      ;;
    claude)
      (cd "$workdir" && run_with_timeout "$TIMEOUT" env -u CLAUDECODE claude -p --verbose --dangerously-skip-permissions --output-format stream-json "$prompt" < /dev/null > "$outfile" 2>"$errfile") || rc=$?
      ;;
    *)
      (cd "$workdir" && run_with_timeout "$TIMEOUT" "$AGENT" -p "$prompt" < /dev/null > "$outfile" 2>"$errfile") || rc=$?
      ;;
  esac

  rm -rf "$workdir"
  return $rc
}

# --- Report lifecycle ---
# Create initial report with all cases as pending.
init_report() {
  local report="${RUN_DIR}/report.md"
  cat > "$report" <<EOF
# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | $TOTAL |
| Passed | - |
| Failed | - |
| Trigger accuracy | - |
| Usage accuracy | - |
| Avg duration | - |
| Avg turns | - |
| Wall time | running... |

## Results by Category

_Pending..._

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
EOF

  for cid in $CASE_IDS; do
    local cjson
    cjson=$(jq -c ".cases[] | select(.id == \"${cid}\")" "$CASES_FILE")
    local ccat
    ccat=$(echo "$cjson" | jq -r '.category')
    echo "| ${cid} | ${ccat} | ⏳ | ⏳ | ⏳ Running | - | - |  |" >> "$report"
  done

  cp "$report" "${SCRIPT_DIR}/reports/${REPORT_NAME}.md"
}

# Atomically update one case's row in the report. Uses mkdir as a portable
# cross-process lock (atomic on all filesystems).
update_report_case() {
  local case_id="$1"
  local result_file="${RUN_DIR}/${case_id}/result.json"
  local report="${RUN_DIR}/report.md"

  # Build the replacement row from result.json
  local new_line
  new_line=$(jq -r '
    "| \(.case_id) | \(.category // "-") | \(if .trigger_pass then "✅" else "❌" end) | \(if .usage_pass then "✅" else "❌" end) | \(if .overall_pass then "✅ PASS" else "❌ FAIL" end) | \((.duration_ms // 0) / 1000 | tostring | split(".") | .[0] + "." + (.[1] // "0" | .[:1]))s | \(.num_turns // 0) | \(.reasons // [] | join("; ")) |"
  ' "$result_file")

  # Acquire lock
  while ! mkdir "${RUN_DIR}/.report.lock" 2>/dev/null; do
    sleep 0.05
  done

  # Replace the matching row
  awk -v id="$case_id" -v line="$new_line" '
    index($0, "| " id " |") == 1 { print line; next }
    { print }
  ' "$report" > "${report}.tmp" && mv "${report}.tmp" "$report"
  cp "$report" "${SCRIPT_DIR}/reports/${REPORT_NAME}.md"

  # Release lock
  rmdir "${RUN_DIR}/.report.lock" 2>/dev/null || true
}

# Regenerate the full report with accurate summary, categories, and failure details.
finalize_report() {
  local elapsed="$1"
  "${SCRIPT_DIR}/report.sh" "$RESULTS_FILE" "$elapsed" > "${RUN_DIR}/report.md"
  cp "${RUN_DIR}/report.md" "${SCRIPT_DIR}/reports/${REPORT_NAME}.md"
}

# --- Per-case logic ---
# Runs a single case end-to-end. Writes result JSON to $CASE_DIR/result.json
# and a status line to $CASE_DIR/status.txt. Safe to call from subshells.
run_single_case() {
  local case_id="$1"
  local case_json
  case_json=$(jq -c ".cases[] | select(.id == \"${case_id}\")" "$CASES_FILE")
  local prompt
  prompt=$(echo "$case_json" | jq -r '.prompt')
  local category
  category=$(echo "$case_json" | jq -r '.category')

  local case_dir="${RUN_DIR}/${case_id}"
  mkdir -p "$case_dir"
  local transcript="${case_dir}/transcript.jsonl"

  local case_start
  case_start=$(date +%s)
  set +e
  run_agent "$prompt" "$transcript"
  local exit_code=$?
  set -e
  local case_elapsed=$(( $(date +%s) - case_start ))

  if [ $exit_code -eq 124 ]; then
    jq -n --arg id "$case_id" --arg cat "$category" --arg prompt "$prompt" \
      --argjson timeout_ms "$((TIMEOUT * 1000))" \
      '{case_id: $id, category: $cat, prompt: $prompt, error: "timeout", trigger_pass: false, usage_pass: false, overall_pass: false, reasons: ["timeout (\($timeout_ms/1000)s)"], presto_calls: 0, curl_calls: 0, skill_loaded: false, presto_cmds: [], duration_ms: $timeout_ms, num_turns: 0}' \
      > "${case_dir}/result.json"
    echo "TIMEOUT (${case_elapsed}s)" > "${case_dir}/status.txt"
    update_report_case "$case_id"
    return
  fi

  # Convert transcript to readable markdown
  "${SCRIPT_DIR}/format_transcript.sh" "$transcript" > "${case_dir}/transcript.md" 2>/dev/null || true

  # Validate
  local result
  result=$("${SCRIPT_DIR}/validate.sh" "$case_json" "$transcript" 2>/dev/null || \
    echo '{"error":"validation_failed","overall_pass":false}')

  # Extract agent's final answer on failure
  local final_answer=""
  if [ "$(echo "$result" | jq -r '.overall_pass')" = "false" ]; then
    final_answer=$(jq -r 'select(.type == "result") | .result // empty' "$transcript" 2>/dev/null | tail -1)
    if [ -z "$final_answer" ]; then
      final_answer=$(jq -r '
        select(.type == "assistant") |
        .message.content[]? |
        select(.type == "text") | .text
      ' "$transcript" 2>/dev/null | tail -1)
    fi
    final_answer=$(echo "$final_answer" | head -c 2000)
  fi

  # Add metadata
  result=$(echo "$result" | jq \
    --arg cat "$category" \
    --arg prompt "$prompt" \
    --arg answer "$final_answer" \
    '. + {category: $cat, prompt: $prompt, agent_response: (if $answer == "" then null else $answer end)}')

  echo "$result" > "${case_dir}/result.json"

  local overall
  overall=$(echo "$result" | jq -r '.overall_pass')
  if [ "$overall" = "true" ]; then
    echo "PASS  (${case_elapsed}s)" > "${case_dir}/status.txt"
  else
    local reasons
    reasons=$(echo "$result" | jq -r '.reasons // [] | join("; ")')
    echo "FAIL  (${case_elapsed}s) (${reasons})" > "${case_dir}/status.txt"
  fi

  # Update the report row for this case
  update_report_case "$case_id"
}

# --- Execution ---

RESULTS_FILE="${RUN_DIR}/results.jsonl"
: > "$RESULTS_FILE"

mkdir -p "${SCRIPT_DIR}/reports"
if [ -n "$SKILL_FILE" ]; then
  REPORT_NAME="${AGENT}-${SKILL_LABEL}"
else
  REPORT_NAME="${AGENT}"
fi

# Create report template with all cases as pending
init_report

if [ "$PARALLEL" -le 1 ]; then
  # --- Sequential mode ---
  for case_id in $CASE_IDS; do
    printf "  %-40s " "$case_id"
    run_single_case "$case_id"

    STATUS=$(cat "${RUN_DIR}/${case_id}/status.txt")
    echo "$STATUS"
  done
else
  # --- Parallel mode ---
  PIDS=()
  PID_CASES=()

  for case_id in $CASE_IDS; do
    run_single_case "$case_id" &
    PIDS+=($!)
    PID_CASES+=("$case_id")

    # When we hit the concurrency limit, wait for one slot to free up
    while [ ${#PIDS[@]} -ge "$PARALLEL" ]; do
      # Poll for any finished PID
      NEW_PIDS=()
      NEW_CASES=()
      for i in "${!PIDS[@]}"; do
        if kill -0 "${PIDS[$i]}" 2>/dev/null; then
          NEW_PIDS+=("${PIDS[$i]}")
          NEW_CASES+=("${PID_CASES[$i]}")
        fi
      done
      if [ ${#NEW_PIDS[@]} -lt ${#PIDS[@]} ]; then
        PIDS=("${NEW_PIDS[@]}")
        PID_CASES=("${NEW_CASES[@]}")
        break
      fi
      sleep 0.2
    done
  done

  # Wait for all remaining jobs
  wait

  # Print results in original case order
  for case_id in $CASE_IDS; do
    STATUS=$(cat "${RUN_DIR}/${case_id}/status.txt" 2>/dev/null || echo "ERROR")
    printf "  %-40s %s\n" "$case_id" "$STATUS"
  done
fi

# --- Aggregate results and finalize ---
PASSED=0
FAILED=0
ERRORS=0

for case_id in $CASE_IDS; do
  STATUS=$(cat "${RUN_DIR}/${case_id}/status.txt" 2>/dev/null || echo "ERROR")
  cat "${RUN_DIR}/${case_id}/result.json" >> "$RESULTS_FILE"

  if [[ "$STATUS" == PASS* ]]; then
    PASSED=$((PASSED + 1))
  elif [[ "$STATUS" == TIMEOUT* ]]; then
    ERRORS=$((ERRORS + 1))
  else
    FAILED=$((FAILED + 1))
  fi
done

SUITE_END=$(date +%s)
SUITE_ELAPSED=$((SUITE_END - SUITE_START))
SUITE_MIN=$((SUITE_ELAPSED / 60))
SUITE_SEC=$((SUITE_ELAPSED % 60))

echo ""
echo "=== Results ==="
echo "Passed: $PASSED / $TOTAL"
echo "Failed: $FAILED / $TOTAL"
if [ $ERRORS -gt 0 ]; then
  echo "Errors: $ERRORS / $TOTAL"
fi
printf "Wall time: %dm%02ds\n" "$SUITE_MIN" "$SUITE_SEC"
echo ""

# Finalize report with accurate summary, categories, and failure details
finalize_report "$SUITE_ELAPSED"
echo "Report: eval/reports/${REPORT_NAME}.md"

# Write summary JSON
jq -s --argjson wall "$SUITE_ELAPSED" '{
  total: length,
  passed: [.[] | select(.overall_pass == true)] | length,
  failed: [.[] | select(.overall_pass == false)] | length,
  trigger_accuracy: (([.[] | select(.trigger_pass == true)] | length) / (length | if . == 0 then 1 else . end) * 100 | floor),
  usage_accuracy: (
    ([.[] | select(.trigger_pass == true and .usage_pass == true)] | length) /
    ([.[] | select(.trigger_pass == true)] | length | if . == 0 then 1 else . end) * 100 | floor
  ),
  wall_time_s: $wall,
  avg_duration_ms: ([.[] | .duration_ms // 0] | if length == 0 then 0 else add / length | floor end),
  avg_turns: ([.[] | .num_turns // 0] | if length == 0 then 0 else add / length * 10 | floor | . / 10 end),
  by_category: (group_by(.category) | map({
    category: .[0].category,
    total: length,
    passed: [.[] | select(.overall_pass == true)] | length
  })),
  failures: [.[] | select(.overall_pass == false) | {case_id, category, reasons, presto_cmds}]
}' "$RESULTS_FILE" > "${RUN_DIR}/summary.json"

echo "Summary: ${RUN_DIR}/summary.json"

# Exit with failure if any cases failed
if [ $FAILED -gt 0 ] || [ $ERRORS -gt 0 ]; then
  exit 1
fi
