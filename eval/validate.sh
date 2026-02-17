#!/usr/bin/env bash
# Validate a single eval case result.
#
# Usage: validate.sh <case_json> <transcript_file>
#
# Parses the agent's stream-json transcript to find Bash tool_use calls
# containing "presto" or "curl", then checks against expectations.

set -euo pipefail

CASE_JSON="$1"
TRANSCRIPT="$2"

CASE_ID=$(echo "$CASE_JSON" | jq -r '.id')
PRESTO_SHOULD=$(echo "$CASE_JSON" | jq -r '.expect.presto.should_invoke // false')
# "unset" means we don't care about curl usage; only check if explicitly specified
CURL_SHOULD=$(echo "$CASE_JSON" | jq -r '.expect.curl.should_invoke // "unset"')

# Extract all Bash tool_use commands from the stream-json transcript.
# Each line is a JSON object; we want assistant messages with tool_use blocks.
# Amp uses .input.cmd, Claude Code uses .input.command — handle both.
# Commands may span multiple lines (backslash continuations), so we collapse
# each command into a single line separated by NUL, then convert to one-per-line.
BASH_CMDS=""
if [ -f "$TRANSCRIPT" ] && [ -s "$TRANSCRIPT" ]; then
  BASH_CMDS=$(grep '"tool_use"' "$TRANSCRIPT" | \
    jq -r '
      .message.content[]? |
      select(.type == "tool_use" and (.name == "Bash" or .name == "bash")) |
      ((.input.cmd // .input.command // empty) | gsub("\\\\\n\\s*"; " ") | gsub("\n"; " "))
    ' 2>/dev/null || true)
fi

# Find presto invocations: matches "presto ..." or "cargo run ..." as the command
# (not "presto" appearing in a file path like /Users/foo/presto).
# We check: command starts with presto, or has "| presto", "&&  presto", or "cargo run".
PRESTO_CMDS=""
PRESTO_CALLS=0
if [ -n "$BASH_CMDS" ]; then
  PRESTO_CMDS=$(echo "$BASH_CMDS" | grep -E '(^|&&|\|\||;|\|)\s*(presto|cargo\s+run)\s' || true)
  if [ -n "$PRESTO_CMDS" ]; then
    PRESTO_CALLS=$(echo "$PRESTO_CMDS" | grep -c . || true)
  fi
fi

# Find curl invocations
CURL_CMDS=""
CURL_CALLS=0
if [ -n "$BASH_CMDS" ]; then
  CURL_CMDS=$(echo "$BASH_CMDS" | grep -E '(^|\s|/)curl(\s|$)' || true)
  if [ -n "$CURL_CMDS" ]; then
    CURL_CALLS=$(echo "$CURL_CMDS" | grep -c . || true)
  fi
fi

# Also check: did the agent load the presto skill?
# Amp uses "skill", Claude Code uses "Skill"
SKILL_LOADED=false
if [ -f "$TRANSCRIPT" ] && grep -qiE '"name":"skill"' "$TRANSCRIPT" 2>/dev/null; then
  if grep -i '"name":"skill"' "$TRANSCRIPT" | jq -r '
    .message.content[]? |
    select(.type == "tool_use" and (.name == "skill" or .name == "Skill")) |
    (.input.name // .input.skill // empty)
  ' 2>/dev/null | grep -qi 'presto'; then
    SKILL_LOADED=true
  fi
fi

TRIGGER_PASS=true
USAGE_PASS=true
REASONS="[]"

add_reason() {
  REASONS=$(echo "$REASONS" | jq --arg r "$1" '. + [$r]')
}

# --- Trigger accuracy ---

if [ "$PRESTO_SHOULD" = "true" ] && [ "$PRESTO_CALLS" -eq 0 ]; then
  TRIGGER_PASS=false
  add_reason "expected presto invocation but none found in Bash commands"
fi

if [ "$PRESTO_SHOULD" = "false" ] && [ "$PRESTO_CALLS" -gt 0 ]; then
  TRIGGER_PASS=false
  add_reason "presto invoked but should not have been (${PRESTO_CALLS} calls)"
fi

if [ "$CURL_SHOULD" != "unset" ]; then
  if [ "$CURL_SHOULD" = "true" ] && [ "$CURL_CALLS" -eq 0 ]; then
    TRIGGER_PASS=false
    add_reason "expected curl invocation but none found"
  fi

  if [ "$CURL_SHOULD" = "false" ] && [ "$CURL_CALLS" -gt 0 ]; then
    TRIGGER_PASS=false
    add_reason "curl invoked but should not have been (${CURL_CALLS} calls)"
  fi
fi

# --- Usage correctness (only when presto was expected AND invoked) ---

if [ "$PRESTO_SHOULD" = "true" ] && [ "$PRESTO_CALLS" -gt 0 ]; then

  # Check URL pattern
  URL_PATTERN=$(echo "$CASE_JSON" | jq -r '.expect.presto.url_pattern // empty')
  if [ -n "$URL_PATTERN" ]; then
    if ! echo "$PRESTO_CMDS" | grep -qE "$URL_PATTERN"; then
      USAGE_PASS=false
      add_reason "no presto command matched url_pattern: ${URL_PATTERN}"
    fi
  fi

  # Check HTTP method
  EXPECTED_METHOD=$(echo "$CASE_JSON" | jq -r '.expect.presto.method // empty')
  if [ -n "$EXPECTED_METHOD" ]; then
    if ! echo "$PRESTO_CMDS" | grep -qiE "(-X|--request)\s+${EXPECTED_METHOD}"; then
      USAGE_PASS=false
      add_reason "expected method ${EXPECTED_METHOD} not found"
    fi
  fi

  # Check has_flag
  HAS_FLAG=$(echo "$CASE_JSON" | jq -r '.expect.presto.has_flag // empty')
  if [ -n "$HAS_FLAG" ]; then
    if ! echo "$PRESTO_CMDS" | grep -qF -- "$HAS_FLAG"; then
      USAGE_PASS=false
      add_reason "expected flag ${HAS_FLAG} not found"
    fi
  fi

  # Check argv_contains (any of the listed strings)
  ARGV_CONTAINS_LEN=$(echo "$CASE_JSON" | jq '.expect.presto.argv_contains // [] | length')
  if [ "$ARGV_CONTAINS_LEN" -gt 0 ]; then
    CONTAINS_MATCH=false
    while IFS= read -r needle; do
      if echo "$PRESTO_CMDS" | grep -qF -- "$needle"; then
        CONTAINS_MATCH=true
        break
      fi
    done < <(echo "$CASE_JSON" | jq -r '.expect.presto.argv_contains[]')
    if [ "$CONTAINS_MATCH" = "false" ]; then
      EXPECTED_LIST=$(echo "$CASE_JSON" | jq -c '.expect.presto.argv_contains')
      USAGE_PASS=false
      add_reason "none of ${EXPECTED_LIST} found in command"
    fi
  fi

  # Check json_checks (jq predicates against --json body)
  JSON_CHECKS_LEN=$(echo "$CASE_JSON" | jq '.expect.presto.json_checks // [] | length')
  if [ "$JSON_CHECKS_LEN" -gt 0 ]; then
    # Extract JSON body from the presto command: value after --json
    # This is tricky with shell quoting; try to grab the quoted string after --json
    JSON_BODY=$(echo "$PRESTO_CMDS" | grep -oE -- "--json '[^']*'" | head -1 | sed "s/--json '//;s/'$//" || true)
    if [ -z "$JSON_BODY" ]; then
      # Try double-quoted
      JSON_BODY=$(echo "$PRESTO_CMDS" | grep -oE -- '--json "[^"]*"' | head -1 | sed 's/--json "//;s/"$//' || true)
    fi

    if [ -n "$JSON_BODY" ]; then
      while IFS= read -r check; do
        RESULT=$(echo "$JSON_BODY" | jq -r "$check" 2>/dev/null || echo "false")
        if [ "$RESULT" != "true" ]; then
          USAGE_PASS=false
          add_reason "json_check failed: ${check}"
        fi
      done < <(echo "$CASE_JSON" | jq -r '.expect.presto.json_checks[]')
    else
      USAGE_PASS=false
      add_reason "no --json body found for json_checks"
    fi
  fi
fi

# --- Compose result ---
OVERALL_PASS=true
if [ "$TRIGGER_PASS" = "false" ] || [ "$USAGE_PASS" = "false" ]; then
  OVERALL_PASS=false
fi

# Capture the actual commands for debugging
PRESTO_CMDS_JSON="[]"
if [ -n "$PRESTO_CMDS" ]; then
  PRESTO_CMDS_JSON=$(echo "$PRESTO_CMDS" | jq -R -s 'split("\n") | map(select(. != ""))')
fi

# --- Extract performance metrics from transcript ---
DURATION_MS=0
NUM_TURNS=0
if [ -f "$TRANSCRIPT" ] && [ -s "$TRANSCRIPT" ]; then
  DURATION_MS=$(jq -r 'select(.type == "result") | .duration_ms // 0' "$TRANSCRIPT" 2>/dev/null | tail -1)
  NUM_TURNS=$(jq -r 'select(.type == "result") | .num_turns // 0' "$TRANSCRIPT" 2>/dev/null | tail -1)
  # Default to 0 if empty
  DURATION_MS=${DURATION_MS:-0}
  NUM_TURNS=${NUM_TURNS:-0}
fi

jq -n \
  --arg case_id "$CASE_ID" \
  --argjson trigger_pass "$TRIGGER_PASS" \
  --argjson usage_pass "$USAGE_PASS" \
  --argjson overall_pass "$OVERALL_PASS" \
  --argjson presto_calls "$PRESTO_CALLS" \
  --argjson curl_calls "$CURL_CALLS" \
  --argjson skill_loaded "$SKILL_LOADED" \
  --argjson reasons "$REASONS" \
  --argjson presto_cmds "$PRESTO_CMDS_JSON" \
  --argjson duration_ms "$DURATION_MS" \
  --argjson num_turns "$NUM_TURNS" \
  '{
    case_id: $case_id,
    trigger_pass: $trigger_pass,
    usage_pass: $usage_pass,
    overall_pass: $overall_pass,
    presto_calls: $presto_calls,
    curl_calls: $curl_calls,
    skill_loaded: $skill_loaded,
    reasons: $reasons,
    presto_cmds: $presto_cmds,
    duration_ms: $duration_ms,
    num_turns: $num_turns
  }'
