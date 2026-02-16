#!/usr/bin/env bash
# Basic Payment Demo — single paid request with presto
#
# Makes a single paid request. The server handles payment automatically:
# presto detects the 402, signs a payment, and retries with credentials.
#
# Flow:
#   1. presto sends POST to paid endpoint → receives 402 + challenge
#   2. Automatically opens a payment channel and signs a voucher
#   3. Retries the request with the payment credential
#   4. Receives the response
#   5. Closes the session (single-use, no persistence)
#
# Prerequisites:
#   - presto installed (`make install` or `cargo install --path .`)
#   - Run `presto login` to connect your Tempo wallet
#
# Usage:
#   ./examples/basic.sh

set -euo pipefail

ENDPOINT="https://openrouter.payments.tempo.xyz/v1/chat/completions"
MODEL="openai/gpt-4o-mini"
PROMPT="Tell me a fortune in one sentence."

echo "=== presto Basic Payment Demo ==="
echo ""
echo "Endpoint: ${ENDPOINT}"
echo "Model:    ${MODEL}"
echo ""

# Ensure wallet is configured
if ! presto whoami 2>/dev/null | grep -q "Ready"; then
  echo "No wallet configured. Running 'presto login'..."
  presto login
  echo ""
fi

echo "--- Wallet ---"
presto whoami 2>/dev/null
echo ""

echo "--- Balance (before) ---"
presto balance 2>/dev/null || echo "(could not fetch balance)"
echo ""

echo "--- Sending single paid request ---"
echo "Prompt: \"${PROMPT}\""
echo "---"

presto -v query -X POST \
  --json "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"${PROMPT}\"}]}" \
  "${ENDPOINT}"

echo ""
echo ""

# Close the session — single-use, no need to persist
presto session close "${ENDPOINT}" 2>/dev/null || true

echo "--- Balance (after) ---"
presto balance 2>/dev/null || echo "(could not fetch balance)"
echo ""
echo "=== Done ==="
