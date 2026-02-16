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
#   - presto installed (`make install` or `cargo install --path .`)
#   - Run `presto login` to connect your Tempo wallet
#
# Usage:
#   ./examples/session-sse.sh

set -euo pipefail

ENDPOINT="https://openrouter.payments.tempo.xyz/v1/chat/completions"
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
if ! presto whoami 2>/dev/null | grep -q "Ready"; then
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
  echo "---"

  presto -v query -X POST \
    --json "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"${PROMPT}\"}],\"stream\":true}" \
    "${ENDPOINT}" 2>/dev/null || true

  echo ""
  echo ""
done

echo "--- Closing session ---"
presto session close "${ENDPOINT}" 2>/dev/null || true
echo ""

echo "--- Balance (after) ---"
presto balance 2>/dev/null || echo "(could not fetch balance)"
echo ""
echo "=== Done ==="
