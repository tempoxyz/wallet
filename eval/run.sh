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
    -h|--help)
      echo "Usage: $0 [--agent amp|claude] [--category CAT] [--case ID] [--skill PATH] [--dry-run] [--timeout SECS]"
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
  # Determine skill install locations
  # Amp: .ai/skills/presto/SKILL.md (repo-local)
  # Claude: ~/.claude/skills/presto/SKILL.md (user-global)
  SKILL_TARGETS=("${REPO_DIR}/.ai/skills/presto/SKILL.md")
  if [ -f "${HOME}/.claude/skills/presto/SKILL.md" ]; then
    SKILL_TARGETS+=("${HOME}/.claude/skills/presto/SKILL.md")
  fi
  # Backup originals and swap
  SKILL_BACKUP="${RUN_DIR}/.skill-backup"
  mkdir -p "$SKILL_BACKUP"
  for target in "${SKILL_TARGETS[@]}"; do
    backup_name=$(echo "$target" | tr '/' '_')
    cp "$target" "${SKILL_BACKUP}/${backup_name}"
    cp "$SKILL_FILE" "$target"
  done
  # Save the variant in the run dir for reference
  cp "$SKILL_FILE" "${RUN_DIR}/SKILL.md"
  # Restore on exit
  trap 'for target in "${SKILL_TARGETS[@]}"; do backup_name=$(echo "$target" | tr "/" "_"); cp "${SKILL_BACKUP}/${backup_name}" "$target" 2>/dev/null || true; done' EXIT
fi

SUITE_START=$(date +%s)

echo "=== Presto Skill Eval ==="
echo "Agent:    $AGENT"
echo "Cases:    $CASES_FILE"
if [ -n "$SKILL_FILE" ]; then
  echo "Skill:    $SKILL_FILE"
fi
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

  # Perl-based fallback: uses SIGALRM without backgrounding the process
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

  case "$AGENT" in
    amp)
      run_with_timeout "$TIMEOUT" amp --stream-json -x "$prompt" < /dev/null > "$outfile" 2>"$errfile"
      ;;
    claude)
      run_with_timeout "$TIMEOUT" env -u CLAUDECODE claude -p --verbose --dangerously-skip-permissions --output-format stream-json "$prompt" < /dev/null > "$outfile" 2>"$errfile"
      ;;
    *)
      run_with_timeout "$TIMEOUT" "$AGENT" -p "$prompt" < /dev/null > "$outfile" 2>"$errfile"
      ;;
  esac
}

# Run cases
PASSED=0
FAILED=0
ERRORS=0
RESULTS_FILE="${RUN_DIR}/results.jsonl"
: > "$RESULTS_FILE"

# Determine report name for live updates
mkdir -p "${SCRIPT_DIR}/reports"
if [ -n "$SKILL_FILE" ]; then
  REPORT_NAME="${AGENT}-${SKILL_LABEL}"
else
  REPORT_NAME="${AGENT}"
fi

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
    jq -n --arg id "$case_id" --arg cat "$CATEGORY_VAL" --arg prompt "$PROMPT" \
      --argjson timeout_ms "$((TIMEOUT * 1000))" \
      '{case_id: $id, category: $cat, prompt: $prompt, error: "timeout", trigger_pass: false, usage_pass: false, overall_pass: false, reasons: ["timeout (\($timeout_ms/1000)s)"], presto_calls: 0, curl_calls: 0, skill_loaded: false, presto_cmds: [], duration_ms: $timeout_ms, num_turns: 0}' >> "$RESULTS_FILE"
    # Update report after timeout too
    ELAPSED=$(($(date +%s) - SUITE_START))
    "${SCRIPT_DIR}/report.sh" "$RESULTS_FILE" "$ELAPSED" > "${RUN_DIR}/report.md"
    cp "${RUN_DIR}/report.md" "${SCRIPT_DIR}/reports/${REPORT_NAME}.md"
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

  # Regenerate report after each case for live progress
  ELAPSED=$(($(date +%s) - SUITE_START))
  "${SCRIPT_DIR}/report.sh" "$RESULTS_FILE" "$ELAPSED" > "${RUN_DIR}/report.md"
  cp "${RUN_DIR}/report.md" "${SCRIPT_DIR}/reports/${REPORT_NAME}.md"
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

# Generate final report with accurate wall time
"${SCRIPT_DIR}/report.sh" "$RESULTS_FILE" "$SUITE_ELAPSED" > "${RUN_DIR}/report.md"
cp "${RUN_DIR}/report.md" "${SCRIPT_DIR}/reports/${REPORT_NAME}.md"
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
