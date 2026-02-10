#!/usr/bin/env bash
#
# End-to-end test for streaming payments via tempoctl.
#
# Prerequisites:
#   1. Local payments server running: cd ai-payments/apps/payments && pnpm dev
#   2. tempoctl configured with a funded wallet on Moderato
#
# Usage:
#   ./examples/stream-e2e.sh                        # 3 vouchers, server-side close (default)
#   ./examples/stream-e2e.sh --close=client          # 3 vouchers, client-side close (on-chain)
#   ./examples/stream-e2e.sh --close=server 5        # 5 vouchers, server-side close
#   ./examples/stream-e2e.sh --close=client 10       # 10 vouchers, client-side close
#   PROXY_URL=https://rpc.payments.tempo.xyz ./examples/stream-e2e.sh 5

set -euo pipefail

# --------------------------------------------------------------------------
# Parse arguments
# --------------------------------------------------------------------------
CLOSE_MODE="server"
VOUCHER_COUNT=""

for arg in "$@"; do
  case "$arg" in
    --close=*) CLOSE_MODE="${arg#--close=}" ;;
    *)         VOUCHER_COUNT="$arg" ;;
  esac
done

VOUCHER_COUNT="${VOUCHER_COUNT:-3}"
PROXY_URL="${PROXY_URL:-http://localhost:8787}"
RPC_ENDPOINT="${PROXY_URL}/rpc"
TEMPOCTL="${TEMPOCTL:-tempoctl}"
PASS=0
FAIL=0
STREAM_STATE="$HOME/Library/Application Support/tempoctl/stream_channels.json"

green()  { printf "\033[32m%s\033[0m\n" "$*"; }
red()    { printf "\033[31m%s\033[0m\n" "$*"; }
yellow() { printf "\033[33m%s\033[0m\n" "$*"; }
bold()   { printf "\033[1m%s\033[0m\n" "$*"; }

pass() { PASS=$((PASS + 1)); green "  ✓ $1"; }
fail() { FAIL=$((FAIL + 1)); red "  ✗ $1"; }

if [ "$CLOSE_MODE" != "server" ] && [ "$CLOSE_MODE" != "client" ]; then
  red "Invalid --close mode: $CLOSE_MODE (must be 'server' or 'client')"
  exit 1
fi

bold "=== Streaming Payments E2E Test ==="
echo "Proxy URL:      $RPC_ENDPOINT"
echo "tempoctl:       $TEMPOCTL"
echo "Voucher count:  $VOUCHER_COUNT"
echo "Close mode:     $CLOSE_MODE"
echo ""

# --------------------------------------------------------------------------
# 0. Sanity: server is reachable
# --------------------------------------------------------------------------
bold "Step 0: Check server is reachable"
if ! curl -sf -o /dev/null --connect-timeout 3 "$PROXY_URL"; then
  red "Server at $PROXY_URL is not reachable. Start it first."
  exit 1
fi
pass "Server is reachable"

# --------------------------------------------------------------------------
# 1. Verify the server issues intent="stream" challenges
# --------------------------------------------------------------------------
bold "Step 1: Verify server issues stream challenges"
WWW_AUTH=$(curl -s -o /dev/null -w '%{http_code}' \
  -D- "$RPC_ENDPOINT" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
  2>/dev/null | grep -i 'www-authenticate' || true)

if echo "$WWW_AUTH" | grep -q 'intent="stream"'; then
  pass "Server returns intent=\"stream\" challenge"
else
  if echo "$WWW_AUTH" | grep -q 'intent="charge"'; then
    red "Server returns intent=\"charge\" — streaming not configured on server."
    red "Update ai-payments/apps/payments/src/index.ts to use mpay.stream()."
    exit 1
  fi
  fail "Could not find WWW-Authenticate header (server may not require payment for this endpoint)"
fi

# --------------------------------------------------------------------------
# 2. Record pre-existing stream state
# --------------------------------------------------------------------------
bold "Step 2: Record pre-existing stream state"
if [ -f "$STREAM_STATE" ]; then
  CHANNELS_BEFORE=$(python3 -c "import sys,json; print(len(json.load(sys.stdin).get('channels',{})))" < "$STREAM_STATE" 2>/dev/null || echo "0")
else
  CHANNELS_BEFORE=0
fi
echo "  Channels before: $CHANNELS_BEFORE"

# --------------------------------------------------------------------------
# 3. Send requests (first opens channel, rest are vouchers)
# --------------------------------------------------------------------------
RPC_METHODS=("eth_blockNumber" "eth_chainId" "net_version")

for i in $(seq 1 "$VOUCHER_COUNT"); do
  METHOD_IDX=$(( (i - 1) % ${#RPC_METHODS[@]} ))
  RPC_METHOD="${RPC_METHODS[$METHOD_IDX]}"

  if [ "$i" -eq 1 ]; then
    bold "Step 3.$i: First request (open channel + voucher)"
  else
    bold "Step 3.$i: Voucher #$i ($RPC_METHOD)"
  fi

  OUTPUT=$($TEMPOCTL -vvv query "$RPC_ENDPOINT" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"$RPC_METHOD\",\"params\":[],\"id\":$i}" 2>&1) || true

  if echo "$OUTPUT" | grep -qi "panic\|fatal\|FAILED"; then
    fail "Request #$i failed"
    echo "$OUTPUT"
    echo ""
    exit 1
  elif echo "$OUTPUT" | grep -q '"error"'; then
    fail "Request #$i got JSON-RPC error"
    echo "$OUTPUT"
    echo ""
    exit 1
  else
    pass "Request #$i completed ($RPC_METHOD)"
  fi
  echo "$OUTPUT"
  echo ""
done

# --------------------------------------------------------------------------
# 4. Verify channel was persisted
# --------------------------------------------------------------------------
bold "Step 4: Verify channel state was persisted"
if [ -f "$STREAM_STATE" ]; then
  CHANNELS_AFTER=$(python3 -c "import sys,json; print(len(json.load(sys.stdin).get('channels',{})))" < "$STREAM_STATE" 2>/dev/null || echo "0")
  if [ "$CHANNELS_AFTER" -gt "0" ]; then
    pass "Stream state file exists with $CHANNELS_AFTER channel(s)"
  else
    fail "Stream state file exists but has no channels"
  fi
else
  fail "Stream state file not found at: $STREAM_STATE"
fi

# --------------------------------------------------------------------------
# 5. Verify cumulative amount increased in state
# --------------------------------------------------------------------------
bold "Step 5: Verify cumulative amount in stream state"
if [ -f "$STREAM_STATE" ]; then
  CUMULATIVE=$(python3 -c "
import sys, json
state = json.load(sys.stdin)
channels = state.get('channels', {})
for key, ch in channels.items():
    cum = int(ch.get('cumulative_amount', '0'))
    print(cum)
    break
" < "$STREAM_STATE" 2>/dev/null || echo "0")
  if [ "$CUMULATIVE" -gt "0" ]; then
    pass "Cumulative amount is $CUMULATIVE (> 0)"
  else
    fail "Cumulative amount is 0 — vouchers may not be incrementing"
  fi
else
  fail "Stream state file not found"
fi

# --------------------------------------------------------------------------
# 6. Close the stream channel
# --------------------------------------------------------------------------
if [ "$CLOSE_MODE" = "server" ]; then
  # --- Server-side close: --close-stream flag sends close via the server ---
  bold "Step 6: Close stream channel (via server, --close-stream)"
  CLOSE_OUTPUT=$($TEMPOCTL -vvv query --close-stream "$RPC_ENDPOINT" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":99}' 2>&1) || true

  if echo "$CLOSE_OUTPUT" | grep -qi "panic\|fatal"; then
    fail "Close failed"
    echo "$CLOSE_OUTPUT"
    exit 1
  elif echo "$CLOSE_OUTPUT" | grep -q '"error"'; then
    fail "Close got JSON-RPC error"
    echo "$CLOSE_OUTPUT"
    exit 1
  elif echo "$CLOSE_OUTPUT" | grep -qi "closing stream channel\|close"; then
    pass "Channel closed via server"
  else
    fail "Close did not trigger channel close"
  fi
  echo "$CLOSE_OUTPUT"
  echo ""

  # Verify channel was removed from state
  bold "Step 7: Verify channel removed from state after close"
  if [ -f "$STREAM_STATE" ]; then
    CHANNELS_AFTER_CLOSE=$(python3 -c "import sys,json; print(len(json.load(sys.stdin).get('channels',{})))" < "$STREAM_STATE" 2>/dev/null || echo "0")
    if [ "$CHANNELS_AFTER_CLOSE" -eq "0" ]; then
      pass "Channel removed from state after close"
    else
      fail "Channel still in state after close ($CHANNELS_AFTER_CLOSE channels)"
    fi
  else
    pass "Stream state file removed (clean close)"
  fi

else
  # --- Client-side close: tempoctl stream close + stream withdraw ---
  bold "Step 6: Close stream channel (client-side, on-chain requestClose)"
  CLOSE_OUTPUT=$($TEMPOCTL -vvv stream close --all 2>&1) || true

  if echo "$CLOSE_OUTPUT" | grep -qi "panic\|fatal"; then
    fail "stream close failed"
    echo "$CLOSE_OUTPUT"
    exit 1
  elif echo "$CLOSE_OUTPUT" | grep -q '"error"'; then
    fail "stream close got JSON-RPC error"
    echo "$CLOSE_OUTPUT"
    exit 1
  elif echo "$CLOSE_OUTPUT" | grep -qi "Requested close\|already has close"; then
    pass "requestClose broadcast on-chain"
  else
    fail "stream close did not trigger requestClose"
  fi
  echo "$CLOSE_OUTPUT"
  echo ""

  # Verify close_requested_at was persisted
  bold "Step 6b: Verify close state persisted"
  if [ -f "$STREAM_STATE" ]; then
    CLOSE_TS=$(python3 -c "
import sys, json
state = json.load(sys.stdin)
channels = state.get('channels', {})
for key, ch in channels.items():
    print(ch.get('close_requested_at', 0))
    break
" < "$STREAM_STATE" 2>/dev/null || echo "0")
    if [ "$CLOSE_TS" -gt "0" ]; then
      pass "close_requested_at persisted ($CLOSE_TS)"
    else
      fail "close_requested_at not set in state"
    fi
  else
    fail "Stream state file not found"
  fi

  # Verify stream list shows closing status
  bold "Step 6c: Verify stream list shows closing status"
  LIST_OUTPUT=$($TEMPOCTL stream list 2>&1)
  echo "$LIST_OUTPUT"
  if echo "$LIST_OUTPUT" | grep -qi "Closing\|READY TO WITHDRAW"; then
    pass "stream list shows close status"
  else
    fail "stream list does not show close status"
  fi
  echo ""

  # Wait for grace period
  bold "Step 7: Wait for 15-minute grace period"
  echo "  Grace period is 15 minutes. Waiting..."
  GRACE_PERIOD=900
  ELAPSED=0
  INTERVAL=30
  while [ "$ELAPSED" -lt "$GRACE_PERIOD" ]; do
    REMAINING=$((GRACE_PERIOD - ELAPSED))
    printf "  %dm %ds remaining...\r" $((REMAINING / 60)) $((REMAINING % 60))
    sleep "$INTERVAL"
    ELAPSED=$((ELAPSED + INTERVAL))
  done
  echo ""
  pass "Grace period elapsed"

  # Withdraw
  bold "Step 8: Withdraw remaining deposit"
  WITHDRAW_OUTPUT=$($TEMPOCTL -vvv stream withdraw --all 2>&1) || true

  if echo "$WITHDRAW_OUTPUT" | grep -qi "panic\|fatal"; then
    fail "stream withdraw failed"
    echo "$WITHDRAW_OUTPUT"
    exit 1
  elif echo "$WITHDRAW_OUTPUT" | grep -q '"error"'; then
    fail "stream withdraw got JSON-RPC error"
    echo "$WITHDRAW_OUTPUT"
    exit 1
  elif echo "$WITHDRAW_OUTPUT" | grep -qi "Withdrew from channel\|already finalized"; then
    pass "Withdraw succeeded"
  elif echo "$WITHDRAW_OUTPUT" | grep -qi "grace period not elapsed"; then
    fail "Grace period not yet elapsed (clock skew?)"
  else
    fail "stream withdraw did not complete"
  fi
  echo "$WITHDRAW_OUTPUT"
  echo ""

  # Verify channel was removed from state
  bold "Step 9: Verify channel removed from state after withdraw"
  if [ -f "$STREAM_STATE" ]; then
    CHANNELS_AFTER_CLOSE=$(python3 -c "import sys,json; print(len(json.load(sys.stdin).get('channels',{})))" < "$STREAM_STATE" 2>/dev/null || echo "0")
    if [ "$CHANNELS_AFTER_CLOSE" -eq "0" ]; then
      pass "Channel removed from state after withdraw"
    else
      fail "Channel still in state after withdraw ($CHANNELS_AFTER_CLOSE channels)"
    fi
  else
    pass "Stream state file removed (clean withdraw)"
  fi
fi

# --------------------------------------------------------------------------
# Summary
# --------------------------------------------------------------------------
echo ""
bold "=== Results ==="
TOTAL=$((PASS + FAIL))
green "  Passed: $PASS/$TOTAL"
if [ "$FAIL" -gt "0" ]; then
  red "  Failed: $FAIL/$TOTAL"
  exit 1
else
  green "  All tests passed!"
fi
