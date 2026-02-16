#!/usr/bin/env bash
# Session Multi-Fetch Demo — multiple paid requests over a single channel
#
# Mirrors the mpp-rs "session/multi-fetch" example: opens one payment
# channel on-chain, then sends multiple requests using off-chain vouchers.
#
# Flow:
#   1. First request:  tempo-walletopens a payment channel on-chain (one gas tx)
#   2. Subsequent requests: reuse the channel with cumulative vouchers (no gas)
#   3. At the end: close the session and settle on-chain
#
# Prerequisites:
#   -  tempo-walletinstalled (`make install` or `cargo install --path .`)
#   - Run ` tempo-walletlogin` to connect your Tempo wallet
#
# Usage:
#   ./examples/session-multi-fetch.sh

set -euo pipefail

STDERR_FILE=$(mktemp)
trap 'rm -f "$STDERR_FILE"' EXIT

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

echo "===  tempo-walletSession Multi-Fetch Demo ==="
echo ""
echo "Endpoint: ${ENDPOINT}"
echo "Model:    ${MODEL}"
echo "Requests: ${#PROMPTS[@]}"
echo ""

# Ensure wallet is configured
if !  tempo-walletwhoami 2>/dev/null | grep -q "Ready"; then
  echo "No wallet configured. Running ' tempo-walletlogin'..."
   tempo-walletlogin
  echo ""
fi

# Clear any existing session for this endpoint
 tempo-walletsession close "${ENDPOINT}" 2>/dev/null || true

echo "--- Wallet ---"
 tempo-walletwhoami 2>/dev/null
echo ""

echo "--- Balance (before) ---"
 tempo-walletbalance 2>/dev/null || echo "(could not fetch balance)"
echo ""

echo "--- Sending ${#PROMPTS[@]} requests over a single session ---"
echo ""

for i in "${!PROMPTS[@]}"; do
  PROMPT="${PROMPTS[$i]}"
  N=$((i + 1))
  echo "[$N/${#PROMPTS[@]}] Prompt: \"${PROMPT}\""

  RESPONSE=$( tempo-wallet-v query -X POST \
    --json "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"${PROMPT}\"}]}" \
    "${ENDPOINT}" 2>"$STDERR_FILE") || true

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
  CUMULATIVE=$(echo "$CLEAN_STDERR" | grep "Session persisted" | sed 's/.*cumulative: \(.*\))/\1/' || true)

  # Extract LLM response
  LLM_RESPONSE=$(echo "$RESPONSE" | jq -r '.choices[0].message.content // empty' 2>/dev/null)
  TOKENS_IN=$(echo "$RESPONSE" | jq -r '.usage.prompt_tokens // empty' 2>/dev/null)
  TOKENS_OUT=$(echo "$RESPONSE" | jq -r '.usage.completion_tokens // empty' 2>/dev/null)

  echo ""
  echo "--- Response ---"
  echo "$LLM_RESPONSE"

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
  echo "  Tokens: ${TOKENS_IN} in / ${TOKENS_OUT} out"
  echo ""
done

echo "--- Closing session ---"
 tempo-walletsession close "${ENDPOINT}" 2>"$STDERR_FILE" || true
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
 tempo-walletbalance 2>/dev/null || echo "(could not fetch balance)"
echo ""
echo "=== Done ==="
