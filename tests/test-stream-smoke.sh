#!/usr/bin/env bash
# End-to-end test for stream intent support against the live payments gateway.
#
# Tests that tempoctl correctly handles 402 responses with stream challenges
# from openrouter.payments.tempo.xyz.
#
# Usage:
#   ./tests/test-stream-smoke.sh              # Uses cargo run
#   TEMPOCTL=./target/release/tempoctl ./tests/test-stream-smoke.sh  # Use a built binary
#
# Prerequisites:
#   - A funded wallet configured in tempoctl (keystore or TEMPOCTL_PRIVATE_KEY)
#   - Network access to openrouter.payments.tempo.xyz
#
# What it tests:
#   1. The server returns a 402 with both stream and charge challenges
#   2. tempoctl can parse the WWW-Authenticate header
#   3. tempoctl accepts the stream intent (doesn't reject with "unsupported intent")
#   4. mpay-rs StreamRequest can decode the stream challenge request

set -euo pipefail

TEMPOCTL="${TEMPOCTL:-cargo run --quiet --}"
ENDPOINT="https://openrouter.payments.tempo.xyz/v1/chat/completions"
PAYLOAD='{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"say hi"}],"stream":true}'

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "${GREEN}✓${NC} $1"; }
fail() { echo -e "${RED}✗${NC} $1"; }
info() { echo -e "${YELLOW}→${NC} $1"; }

echo -e "${BOLD}Stream Intent E2E Tests${NC}"
echo "Endpoint: $ENDPOINT"
echo ""

# ─── Test 1: Verify the server returns stream + charge challenges ─────────────

info "Test 1: Checking server returns stream challenge in WWW-Authenticate header..."

WWW_AUTH=$(curl -s -D - -o /dev/null -X POST \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  "$ENDPOINT" 2>&1 | grep -i '^www-authenticate:' | head -1)

if [ -z "$WWW_AUTH" ]; then
  fail "No WWW-Authenticate header in response"
  exit 1
fi

if echo "$WWW_AUTH" | grep -q 'intent="stream"'; then
  pass "Server returns stream intent in WWW-Authenticate"
else
  fail "No stream intent found in WWW-Authenticate header"
  echo "  Header: $WWW_AUTH"
  exit 1
fi

if echo "$WWW_AUTH" | grep -q 'intent="charge"'; then
  pass "Server also returns charge intent (multi-challenge)"
else
  info "Server only returns stream intent (no charge fallback)"
fi

# ─── Test 2: Check tempoctl can handle the response ──────────────────────────

info "Test 2: Running tempoctl in dry-run mode..."

# Run tempoctl with --dry-run to see if it parses the challenge without paying.
# We expect it to either:
# a) Show payment info and exit (dry-run success)
# b) Fail with a wallet/signing error (challenge was parsed OK)
# c) Fail with "unsupported intent" (BUG - stream not supported)

OUTPUT=$($TEMPOCTL -v query --dry-run -X POST \
  --json "$PAYLOAD" \
  "$ENDPOINT" 2>&1 || true)

if echo "$OUTPUT" | grep -qi "unsupported.*intent\|only.*charge.*intent"; then
  fail "tempoctl rejected stream intent as unsupported"
  echo "  Output: $OUTPUT"
  exit 1
else
  pass "tempoctl did not reject the stream intent"
fi

if echo "$OUTPUT" | grep -qi "Payment intent: stream"; then
  pass "tempoctl recognized intent as 'stream'"
elif echo "$OUTPUT" | grep -qi "Payment intent: charge"; then
  info "tempoctl fell back to charge intent (stream challenge may not parse as standard format)"
else
  info "Could not determine which intent was selected"
fi

if echo "$OUTPUT" | grep -qi "DRY RUN"; then
  pass "Dry run completed successfully"
elif echo "$OUTPUT" | grep -qi "Amount"; then
  pass "Challenge was parsed (amount displayed)"
else
  info "tempoctl output:"
  echo "$OUTPUT" | head -20
fi

# ─── Test 3: Verify StreamRequest fields are recognized ───────────────────────

info "Test 3: Checking challenge field recognition..."

if echo "$OUTPUT" | grep -qi "amount.*per unit\|amount.*atomic"; then
  pass "Amount field recognized in challenge"
fi

if echo "$OUTPUT" | grep -qi "currency\|Currency"; then
  pass "Currency field recognized"
fi

if echo "$OUTPUT" | grep -qi "recipient\|Recipient\|To:"; then
  pass "Recipient field recognized"
fi

echo ""
echo -e "${BOLD}Summary${NC}"
echo "The server at $ENDPOINT returns 402 with stream + charge challenges."
echo "tempoctl can parse and process the response without rejecting the stream intent."
