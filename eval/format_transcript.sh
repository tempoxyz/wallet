#!/usr/bin/env bash
# Convert a stream-json transcript to readable markdown.
#
# Usage: format_transcript.sh <transcript.jsonl> > transcript.md

set -euo pipefail

FILE="$1"

if [ ! -f "$FILE" ]; then
  echo "Error: file not found: $FILE" >&2
  exit 1
fi

echo "# Agent Transcript"
echo ""

while IFS= read -r line; do
  TYPE=$(echo "$line" | jq -r '.type // empty' 2>/dev/null)

  case "$TYPE" in
    system)
      MODEL=$(echo "$line" | jq -r '.model // empty' 2>/dev/null)
      if [ -n "$MODEL" ]; then
        echo "**Model:** $MODEL"
        echo ""
      fi
      ;;

    user)
      # Could be a user prompt or a tool result
      CONTENT=$(echo "$line" | jq -r '.message.content' 2>/dev/null)

      # Check if it's a text prompt
      TEXT=$(echo "$line" | jq -r '.message.content[]? | select(.type == "text") | .text' 2>/dev/null || true)
      if [ -n "$TEXT" ]; then
        echo "## User"
        echo ""
        echo "$TEXT"
        echo ""
        continue
      fi

      # Check if it's a tool result
      TOOL_RESULT=$(echo "$line" | jq -r '.message.content[]? | select(.type == "tool_result") | .content' 2>/dev/null || true)
      if [ -n "$TOOL_RESULT" ]; then
        echo "<details>"
        echo "<summary>Tool Result</summary>"
        echo ""
        echo '```'
        # Try to parse as JSON for pretty output, fall back to raw
        echo "$TOOL_RESULT" | jq -r '
          if type == "string" then
            (try (fromjson | "Exit: \(.exitCode // "n/a")\n\(.output // .stdout // .)") catch .)
          else
            tostring
          end
        ' 2>/dev/null || echo "$TOOL_RESULT"
        echo '```'
        echo "</details>"
        echo ""
      fi
      ;;

    assistant)
      echo "## Assistant"
      echo ""
      # Process each content block
      echo "$line" | jq -c '.message.content[]?' 2>/dev/null | while IFS= read -r block; do
        BLOCK_TYPE=$(echo "$block" | jq -r '.type' 2>/dev/null)
        case "$BLOCK_TYPE" in
          text)
            echo "$block" | jq -r '.text' 2>/dev/null
            echo ""
            ;;
          tool_use)
            TOOL_NAME=$(echo "$block" | jq -r '.name' 2>/dev/null)
            TOOL_INPUT=$(echo "$block" | jq -r '.input | to_entries | map("\(.key): \(.value)") | join("\n")' 2>/dev/null || true)
            echo "**→ $TOOL_NAME**"
            echo '```'
            echo "$TOOL_INPUT"
            echo '```'
            echo ""
            ;;
        esac
      done
      ;;

    result)
      echo "---"
      echo ""
      RESULT=$(echo "$line" | jq -r '.result // empty' 2>/dev/null)
      DURATION=$(echo "$line" | jq -r '.duration_ms // empty' 2>/dev/null)
      TURNS=$(echo "$line" | jq -r '.num_turns // empty' 2>/dev/null)
      if [ -n "$RESULT" ]; then
        echo "**Final Answer:**"
        echo ""
        echo "$RESULT"
        echo ""
      fi
      if [ -n "$DURATION" ]; then
        SECS=$((DURATION / 1000))
        echo "_${TURNS} turns, ${SECS}s_"
      fi
      ;;
  esac
done < "$FILE"
