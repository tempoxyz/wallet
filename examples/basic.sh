#!/usr/bin/env bash
# Basic Payment Demo — single paid request with tempo-wallet
#
# Makes a single paid request using the charge intent (pay-per-request).
#  tempo-walletdetects the 402, signs an on-chain payment, and retries with credentials.
#
# Flow:
#   1.  tempo-walletsends POST to paid endpoint → receives 402 + challenge
#   2. Automatically signs a payment transaction on-chain
#   3. Retries the request with the payment credential
#   4. Receives the response
#
# No payment channels, no sessions — just a single transaction per request.
#
# Prerequisites:
#   -  tempo-walletinstalled (`make install`)
#   - Run ` tempo-walletlogin` to connect your Tempo wallet
#
# Usage:
#   ./examples/basic.sh [PROMPT]

set -euo pipefail

STDERR_FILE=$(mktemp)
trap 'rm -f "$STDERR_FILE"' EXIT

ENDPOINT="https://openai.mpp.tempo.xyz/v1/chat/completions"
MODEL="gpt-4o-mini"
PROMPT="${1:-Tell me a fortune in one sentence.}"

echo "===  tempo-walletBasic Payment Demo ==="
echo ""
echo "Endpoint: ${ENDPOINT}"
echo "Model:    ${MODEL}"
echo "Prompt:   \"${PROMPT}\""
echo ""

# Ensure wallet is configured
if !  tempo-walletwhoami 2>/dev/null | grep -q "Wallet:"; then
  echo "No wallet configured. Running ' tempo-walletlogin'..."
   tempo-walletlogin
  echo ""
fi

echo "--- Sending paid request ---"

RESPONSE=$( tempo-wallet-v -X POST \
  --json "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"${PROMPT}\"}]}" \
  "${ENDPOINT}" 2>"$STDERR_FILE")

STDERR=$(cat "$STDERR_FILE")

# Extract payment details from stderr (strip ANSI/OSC escape sequences)
CLEAN_STDERR=$(echo "$STDERR" | sed $'s/\x1b[^m]*m//g' | sed $'s/\x1b\\][^\x1b]*\x1b\\\\//g')
PAID_LINE=$(echo "$CLEAN_STDERR" | grep "^Paid " || true)
AMOUNT=$(echo "$CLEAN_STDERR" | grep "Amount:" | head -1 | awk '{print $2}')
METHOD=$(echo "$CLEAN_STDERR" | grep "Payment method:" | awk '{print $NF}')
INTENT=$(echo "$CLEAN_STDERR" | grep "Payment intent:" | awk '{print $NF}')

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
echo "  Amount: ${AMOUNT} atomic units"
if [ -n "$PAID_LINE" ]; then
  echo "  ${PAID_LINE}"
fi
echo "  Tokens: ${TOKENS_IN} in / ${TOKENS_OUT} out"
echo ""
echo "=== Done ==="
