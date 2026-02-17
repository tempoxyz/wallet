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
#   ./eval/run.sh --dry-run                    # Show what would run
#
# Environment:
#   EVAL_TIMEOUT  - Per-case timeout in seconds (default: 120)
#   EVAL_CASES    - Path to cases file (default: eval/cases/cases.json)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Defaults
AGENT="amp"
CATEGORY=""
CASE_FILTER=""
DRY_RUN=false
TIMEOUT="${EVAL_TIMEOUT:-120}"
CASES_FILE="${EVAL_CASES:-${SCRIPT_DIR}/cases/cases.json}"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --agent)     AGENT="$2"; shift 2 ;;
    --category)  CATEGORY="$2"; shift 2 ;;
    --case)      CASE_FILTER="$2"; shift 2 ;;
    --dry-run)   DRY_RUN=true; shift ;;
    --timeout)   TIMEOUT="$2"; shift 2 ;;
    --cases)     CASES_FILE="$2"; shift 2 ;;
    -h|--help)
      echo "Usage: $0 [--agent amp|claude] [--category CAT] [--case ID] [--dry-run] [--timeout SECS]"
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

# Resolve timeout command: timeout (Linux), gtimeout (brew coreutils), or builtin fallback
if command -v timeout &>/dev/null; then
  TIMEOUT_CMD="timeout"
elif command -v gtimeout &>/dev/null; then
  TIMEOUT_CMD="gtimeout"
else
  TIMEOUT_CMD="builtin"
fi

# Create run directory
RUN_ID="$(date +%Y%m%d-%H%M%S)-${AGENT}"
RUN_DIR="${SCRIPT_DIR}/runs/${RUN_ID}"
mkdir -p "$RUN_DIR"

echo "=== Presto Skill Eval ==="
echo "Agent:    $AGENT"
echo "Cases:    $CASES_FILE"
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
# otherwise a portable bash fallback that backgrounds the process and kills it.
# Returns 124 on timeout (matching GNU coreutils convention).
run_with_timeout() {
  local secs="$1"
  shift

  if [ "$TIMEOUT_CMD" != "builtin" ]; then
    $TIMEOUT_CMD "${secs}s" "$@"
    return $?
  fi

  # Portable fallback: run in background, kill after deadline
  "$@" &
  local pid=$!

  (
    sleep "$secs"
    kill "$pid" 2>/dev/null
  ) &
  local watchdog=$!

  wait "$pid" 2>/dev/null
  local exit_code=$?

  # Clean up watchdog
  kill "$watchdog" 2>/dev/null
  wait "$watchdog" 2>/dev/null

  # If the process was killed by our watchdog, return 124
  if [ $exit_code -ge 128 ]; then
    return 124
  fi
  return $exit_code
}

run_agent() {
  local prompt="$1"
  local outfile="$2"

  case "$AGENT" in
    amp)
      run_with_timeout "$TIMEOUT" amp --stream-json -x "$prompt" > "$outfile" 2>&1
      ;;
    claude)
      run_with_timeout "$TIMEOUT" env -u CLAUDECODE claude -p --verbose --dangerously-skip-permissions --output-format stream-json "$prompt" > "$outfile" 2>&1
      ;;
    *)
      run_with_timeout "$TIMEOUT" "$AGENT" -p "$prompt" > "$outfile" 2>&1
      ;;
  esac
}

# Run cases
PASSED=0
FAILED=0
ERRORS=0
RESULTS_FILE="${RUN_DIR}/results.jsonl"
: > "$RESULTS_FILE"

for case_id in $CASE_IDS; do
  CASE_JSON=$(jq -c ".cases[] | select(.id == \"${case_id}\")" "$CASES_FILE")
  PROMPT=$(echo "$CASE_JSON" | jq -r '.prompt')
  CATEGORY_VAL=$(echo "$CASE_JSON" | jq -r '.category')

  CASE_DIR="${RUN_DIR}/${case_id}"
  mkdir -p "$CASE_DIR"
  TRANSCRIPT="${CASE_DIR}/transcript.jsonl"

  printf "  %-40s " "$case_id"

  set +e
  run_agent "$PROMPT" "$TRANSCRIPT"
  EXIT_CODE=$?
  set -e

  if [ $EXIT_CODE -eq 124 ]; then
    printf "TIMEOUT\n"
    ERRORS=$((ERRORS + 1))
    jq -n --arg id "$case_id" --arg cat "$CATEGORY_VAL" \
      '{case_id: $id, category: $cat, error: "timeout", overall_pass: false}' >> "$RESULTS_FILE"
    continue
  fi

  # Convert transcript to readable markdown
  "${SCRIPT_DIR}/format_transcript.sh" "$TRANSCRIPT" > "${CASE_DIR}/transcript.md" 2>/dev/null || true

  # Validate against the stream-json transcript
  RESULT=$("${SCRIPT_DIR}/validate.sh" "$CASE_JSON" "$TRANSCRIPT" 2>/dev/null || \
    echo '{"error":"validation_failed","overall_pass":false}')

  # Extract agent's final answer from transcript for failure diagnosis
  FINAL_ANSWER=""
  if [ "$(echo "$RESULT" | jq -r '.overall_pass')" = "false" ]; then
    # Amp: look for result.result; Claude: last assistant text block
    FINAL_ANSWER=$(jq -r '
      select(.type == "result") | .result // empty
    ' "$TRANSCRIPT" 2>/dev/null | tail -1)
    if [ -z "$FINAL_ANSWER" ]; then
      FINAL_ANSWER=$(jq -r '
        select(.type == "assistant") |
        .message.content[]? |
        select(.type == "text") | .text
      ' "$TRANSCRIPT" 2>/dev/null | tail -1)
    fi
    # Truncate to keep results reasonable
    FINAL_ANSWER=$(echo "$FINAL_ANSWER" | head -c 2000)
  fi

  # Add metadata
  RESULT=$(echo "$RESULT" | jq \
    --arg cat "$CATEGORY_VAL" \
    --arg prompt "$PROMPT" \
    --arg answer "$FINAL_ANSWER" \
    '. + {category: $cat, prompt: $prompt, agent_response: (if $answer == "" then null else $answer end)}')

  echo "$RESULT" >> "$RESULTS_FILE"

  OVERALL=$(echo "$RESULT" | jq -r '.overall_pass')
  if [ "$OVERALL" = "true" ]; then
    printf "PASS\n"
    PASSED=$((PASSED + 1))
  else
    REASONS=$(echo "$RESULT" | jq -r '.reasons // [] | join("; ")')
    printf "FAIL  (%s)\n" "$REASONS"
    FAILED=$((FAILED + 1))
  fi
done

echo ""
echo "=== Results ==="
echo "Passed: $PASSED / $TOTAL"
echo "Failed: $FAILED / $TOTAL"
if [ $ERRORS -gt 0 ]; then
  echo "Errors: $ERRORS / $TOTAL"
fi
echo ""

# Generate report
"${SCRIPT_DIR}/report.sh" "$RESULTS_FILE" > "${RUN_DIR}/report.md"
# Copy as latest report for this agent
cp "${RUN_DIR}/report.md" "${SCRIPT_DIR}/report-${AGENT}.md"
echo "Report: eval/report-${AGENT}.md"

# Write summary JSON
jq -s '{
  total: length,
  passed: [.[] | select(.overall_pass == true)] | length,
  failed: [.[] | select(.overall_pass == false)] | length,
  trigger_accuracy: (([.[] | select(.trigger_pass == true)] | length) / (length | if . == 0 then 1 else . end) * 100 | floor),
  usage_accuracy: (
    ([.[] | select(.trigger_pass == true and .usage_pass == true)] | length) /
    ([.[] | select(.trigger_pass == true)] | length | if . == 0 then 1 else . end) * 100 | floor
  ),
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
