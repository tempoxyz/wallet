#!/usr/bin/env bash
# Session SSE Demo — pay-per-token streaming with presto
#
# Mirrors the mpp-rs "session/sse" example: opens a payment channel,
# streams SSE tokens to stdout with per-token vouchers, and settles.
#
# Flow:
#   1. First request: presto opens a payment channel on-chain (one gas tx)
#   2. Streams SSE tokens to stdout, handling mid-stream voucher top-ups
#   3. Subsequent requests: reuse the channel (no gas)
#   4. At the end: close the session and settle on-chain
#
# Prerequisites:
#   - presto installed (`make install`)
#   - Run `presto login` to connect your Tempo wallet
#
# Usage:
#   ./examples/session-sse.sh

set -euo pipefail

STDERR_FILE=$(mktemp)
trap 'rm -f "$STDERR_FILE"' EXIT

ENDPOINT="https://openrouter.mpp.tempo.xyz/v1/chat/completions"
MODEL="openai/gpt-4o-mini"

PROMPTS=(
  "What is a payment channel in one sentence?"
  "What is the capital of France?"
  "Explain SSE in 10 words."
)

echo "=== presto Session SSE Demo ==="
echo ""
echo "Endpoint: ${ENDPOINT}"
echo "Model:    ${MODEL}"
echo "Requests: ${#PROMPTS[@]}"
echo ""

# Ensure wallet is configured
if ! presto whoami 2>/dev/null | grep -q "Wallet:"; then
  echo "No wallet configured. Running 'presto login'..."
  presto login
  echo ""
fi

# Clear any existing session for this endpoint
presto session close "${ENDPOINT}" 2>/dev/null || true

echo "--- Wallet ---"
presto whoami 2>/dev/null
echo ""

echo "--- Balance (before) ---"
presto balance 2>/dev/null || echo "(could not fetch balance)"
echo ""

echo "--- Streaming ${#PROMPTS[@]} requests over a single session ---"
echo ""

for i in "${!PROMPTS[@]}"; do
  PROMPT="${PROMPTS[$i]}"
  N=$((i + 1))
  echo "[$N/${#PROMPTS[@]}] Prompt: \"${PROMPT}\""

  presto -v -X POST \
    --json "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"${PROMPT}\"}],\"stream\":true}" \
    "${ENDPOINT}" 2>"$STDERR_FILE" || true

  STDERR=$(cat "$STDERR_FILE")

  # Extract payment details from stderr (strip ANSI/OSC escape sequences)
  CLEAN_STDERR=$(echo "$STDERR" | sed $'s/\x1b[^m]*m//g' | sed $'s/\x1b\\][^\x1b]*\x1b\\\\//g')
  TX_HASH=$(echo "$CLEAN_STDERR" | grep "Channel open tx:" | awk '{print $NF}' || true)
  if [ -n "$TX_HASH" ] && [ -z "${OPEN_TX:-}" ]; then
    OPEN_TX="$TX_HASH"
  fi
  COST=$(echo "$CLEAN_STDERR" | grep "Cost per request:" | awk '{print $(NF-2)}' || true)
  METHOD=$(echo "$CLEAN_STDERR" | grep "Payment method:" | awk '{print $NF}' || true)
  INTENT=$(echo "$CLEAN_STDERR" | grep "Payment intent:" | awk '{print $NF}' || true)
  CUMULATIVE=$(echo "$CLEAN_STDERR" | grep "Session persisted" | tail -1 | sed 's/.*cumulative: \(.*\))/\1/' || true)

  echo ""
  echo "--- Payment ---"
  echo "  Intent: ${INTENT}"
  echo "  Method: ${METHOD}"
  echo "  Cost:   ${COST} atomic units"
  if [ -n "$TX_HASH" ]; then
    echo "  TX:     ${TX_HASH}"
  fi
  if [ -n "$CUMULATIVE" ]; then
    echo "  Total:  ${CUMULATIVE}"
  fi
  echo ""
done

echo "--- Closing session ---"
presto session close "${ENDPOINT}" 2>"$STDERR_FILE" || true
CLOSE_STDERR=$(cat "$STDERR_FILE")
CLEAN_CLOSE=$(echo "$CLOSE_STDERR" | sed $'s/\x1b[^m]*m//g' | sed $'s/\x1b\\][^\x1b]*\x1b\\\\//g')
CLOSE_TX=$(echo "$CLEAN_CLOSE" | grep "Channel settled:" | awk '{print $NF}' || true)
echo ""

echo "--- Channel ---"
if [ -n "${OPEN_TX:-}" ]; then
  echo "  Open:   ${OPEN_TX}"
fi
if [ -n "$CLOSE_TX" ]; then
  echo "  Settle: ${CLOSE_TX}"
else
  echo "  Settle: pending (server did not return a receipt)"
fi
echo ""

echo "--- Balance (after) ---"
presto balance 2>/dev/null || echo "(could not fetch balance)"
echo ""
echo "=== Done ==="
