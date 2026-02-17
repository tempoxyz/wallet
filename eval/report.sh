#!/usr/bin/env bash
# Generate a markdown report from eval results.
#
# Usage: report.sh <results.jsonl>

set -euo pipefail

RESULTS_FILE="$1"

if [ ! -f "$RESULTS_FILE" ]; then
  echo "Error: results file not found: $RESULTS_FILE" >&2
  exit 1
fi

TOTAL=$(jq -s 'length' "$RESULTS_FILE")
PASSED=$(jq -s '[.[] | select(.overall_pass == true)] | length' "$RESULTS_FILE")
FAILED=$(jq -s '[.[] | select(.overall_pass == false)] | length' "$RESULTS_FILE")
TRIGGER_ACC=$(jq -s '([.[] | select(.trigger_pass == true)] | length) as $tp | ($tp / length * 100 | floor)' "$RESULTS_FILE")
USAGE_ACC=$(jq -s '
  ([.[] | select(.trigger_pass == true)] | length) as $triggered |
  ([.[] | select(.trigger_pass == true and .usage_pass == true)] | length) as $correct |
  if $triggered == 0 then 0 else ($correct / $triggered * 100 | floor) end
' "$RESULTS_FILE")

cat <<EOF
#  Tempo WalletSkill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | $TOTAL |
| Passed | $PASSED |
| Failed | $FAILED |
| Trigger accuracy | ${TRIGGER_ACC}% |
| Usage accuracy | ${USAGE_ACC}% |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
EOF

jq -s '
  group_by(.category) |
  map({
    cat: .[0].category,
    passed: ([.[] | select(.overall_pass == true)] | length),
    total: length
  }) |
  .[] |
  "| \(.cat) | \(.passed) | \(.total) | \(.passed * 100 / .total | floor)% |"
' -r "$RESULTS_FILE"

echo ""
echo "## All Cases"
echo ""
echo "| Case | Category | Trigger | Usage | Result | Notes |"
echo "|------|----------|---------|-------|--------|-------|"

jq -s '.[] |
  "| \(.case_id) | \(.category // "-") | \(if .trigger_pass then "✅" else "❌" end) | \(if .usage_pass then "✅" else "❌" end) | \(if .overall_pass then "✅ PASS" else "❌ FAIL" end) | \(.reasons // [] | join("; ")) |"
' -r "$RESULTS_FILE"

# Show failures detail
FAIL_COUNT=$(jq -s '[.[] | select(.overall_pass == false)] | length' "$RESULTS_FILE")
if [ "$FAIL_COUNT" -gt 0 ]; then
  echo ""
  echo "## Failures"
  echo ""
  jq -s '.[] | select(.overall_pass == false) |
    "### \(.case_id)\n\n**Prompt:** \(.prompt // "n/a")\n\n**Reasons:**\n\(.reasons // [] | map("- " + .) | join("\n"))\n\n** Tempo Walletcalls:** \(.presto_calls // 0) | **Curl calls:** \(.curl_calls // 0)\n\(if .agent_response then "\n**Agent response:**\n> \(.agent_response | gsub("\n"; "\n> "))\n" else "" end)"
  ' -r "$RESULTS_FILE"
fi
