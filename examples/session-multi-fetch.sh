#!/usr/bin/env bash
# Session Multi-Fetch Demo — multiple paid requests over a single channel
#
# Mirrors the mpp-rs "session/multi-fetch" example: opens one payment
# channel on-chain, then sends multiple requests using off-chain vouchers.
#
# Flow:
#   1. First request: presto opens a payment channel on-chain (one gas tx)
#   2. Subsequent requests: reuse the channel with cumulative vouchers (no gas)
#   3. At the end: close the session and settle on-chain
#
# Prerequisites:
#   - presto installed (`make install` or `cargo install --path .`)
#   - Run `presto login` to connect your Tempo wallet
#
# Usage:
#   ./examples/session-multi-fetch.sh

set -euo pipefail

ENDPOINT="https://openrouter.payments.tempo.xyz/v1/chat/completions"
MODEL="openai/gpt-4o-mini"

# 9 short prompts — one request per prompt, all over a single session channel
PROMPTS=(
  "What is 2+2?"
  "Name a primary color."
  "What planet is closest to the sun?"
  "What is the boiling point of water in Celsius?"
  "Name a mammal that can fly."
  "What is the chemical symbol for gold?"
  "How many continents are there?"
  "What year did the Titanic sink?"
  "What is the speed of light in km/s?"
)

echo "=== presto Session Multi-Fetch Demo ==="
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

echo "--- Sending ${#PROMPTS[@]} requests over a single session ---"
echo ""

for i in "${!PROMPTS[@]}"; do
  PROMPT="${PROMPTS[$i]}"
  N=$((i + 1))
  echo "[$N/${#PROMPTS[@]}] Prompt: \"${PROMPT}\""
  echo "---"

  presto -v query -X POST \
    --json "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"${PROMPT}\"}]}" \
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
