#!/usr/bin/env bash
#
# Remote E2E test: verify tempo can auto-install wallet from
# the production manifest at https://cli.tempo.xyz.
#
# This test requires network access and a published wallet release.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLI_DIR="$(dirname "$SCRIPT_DIR")"
TEST_DIR="/tmp/tempo-remote-e2e-test"

RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
RESET='\033[0m'

step() { echo -e "\n${BOLD}==> $1${RESET}"; }
pass() { echo -e "${GREEN}✓ $1${RESET}"; }
fail() { echo -e "${RED}✗ $1${RESET}"; exit 1; }

# ------------------------------------------------------------------
step "Cleaning previous test state"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"

# ------------------------------------------------------------------
step "Building tempo CLI (release)"
cd "$CLI_DIR"
cargo build --release --bin tempo
TEMPO_BIN="$CLI_DIR/target/release/tempo"
pass "Built $TEMPO_BIN"

# ------------------------------------------------------------------
step "Setting up isolated environment"
TEMPO_HOME="$TEST_DIR/home"
BIN_DIR="$TEMPO_HOME/bin"
mkdir -p "$BIN_DIR"
cp "$TEMPO_BIN" "$BIN_DIR/tempo"
pass "Installed tempo to $BIN_DIR"

# ------------------------------------------------------------------
step "Test 1: auto-install wallet via remote manifest"
echo "Triggering auto-install by running: tempo wallet --version"

VERSION_OUT=$(TEMPO_HOME="$TEMPO_HOME" PATH="$BIN_DIR:/usr/bin:/bin" "$BIN_DIR/tempo" wallet --version 2>&1) || true
echo "$VERSION_OUT"

if [ -f "$BIN_DIR/tempo-wallet" ]; then
    pass "auto-install: tempo-wallet binary downloaded"
else
    fail "auto-install: tempo-wallet not found in $BIN_DIR"
fi

if echo "$VERSION_OUT" | grep -q "tempo-wallet"; then
    pass "auto-install: tempo wallet --version shows version"
else
    fail "auto-install: unexpected version output: $VERSION_OUT"
fi

# ------------------------------------------------------------------
step "Test 2: tempo wallet help"
HELP_OUT=$(TEMPO_HOME="$TEMPO_HOME" PATH="$BIN_DIR:/usr/bin:/bin" "$BIN_DIR/tempo" wallet help 2>&1)
echo "$HELP_OUT"

if echo "$HELP_OUT" | grep -q "tempo wallet"; then
    pass "help: shows 'tempo wallet' usage"
else
    fail "help: expected 'tempo wallet' in output"
fi

# ------------------------------------------------------------------
step "Test 3: tempo wallet services"
SERVICES_OUT=$(TEMPO_HOME="$TEMPO_HOME" PATH="$BIN_DIR:/usr/bin:/bin" "$BIN_DIR/tempo" wallet services 2>&1) || true
echo "$SERVICES_OUT"

if echo "$SERVICES_OUT" | grep -qi "service\|available\|name"; then
    pass "services: returned service directory"
else
    # Services may require auth or network; non-fatal
    echo -e "${BOLD}note:${RESET} services output was unexpected but auto-install itself succeeded"
fi

# ------------------------------------------------------------------
step "Test 4: explicit add with baked-in manifest"
rm -f "$BIN_DIR/tempo-wallet"

TEMPO_HOME="$TEMPO_HOME" "$BIN_DIR/tempo" add wallet
echo ""

if [ -f "$BIN_DIR/tempo-wallet" ]; then
    pass "explicit add: tempo-wallet installed"
else
    fail "explicit add: tempo-wallet not found in $BIN_DIR"
fi

TEMPO_HOME="$TEMPO_HOME" "$BIN_DIR/tempo" wallet --version && pass "explicit add: tempo wallet runs" || fail "explicit add: tempo wallet failed"

# ------------------------------------------------------------------
step "Test 5: auto-update via stale state.json"

# Set up an isolated HOME (no TEMPO_HOME so auto-update fires)
AUTO_HOME="$TEST_DIR/home-autoupdate"
AUTO_BIN="$AUTO_HOME/.local/bin"
mkdir -p "$AUTO_BIN"

# Copy tempo and the already-installed wallet binary
cp "$TEMPO_BIN" "$AUTO_BIN/tempo"
cp "$BIN_DIR/tempo-wallet" "$AUTO_BIN/tempo-wallet"
chmod +x "$AUTO_BIN/tempo" "$AUTO_BIN/tempo-wallet"

# Determine platform-native state dir
case "$(uname -s)" in
    Darwin) AUTO_STATE_DIR="$AUTO_HOME/Library/Application Support/tempo" ;;
    *)      AUTO_STATE_DIR="$AUTO_HOME/.local/share/tempo" ;;
esac
mkdir -p "$AUTO_STATE_DIR"

# Get the real current version from the manifest
CURRENT_VERSION=$(curl -fsSL https://cli.tempo.xyz/extensions/tempo-wallet/manifest.json | grep '"version"' | head -1 | sed 's/.*"version".*"\(.*\)".*/\1/')
echo "Current manifest version: $CURRENT_VERSION"

# Write stale state.json with a fake old version (checked_at=0 forces re-check)
cat > "$AUTO_STATE_DIR/state.json" <<EOF
{
  "extensions": {
    "wallet": {
      "checked_at": 0,
      "installed_version": "v0.0.0-stale"
    }
  }
}
EOF

# Run wallet — auto-update should detect version mismatch and update
AUTO_OUTPUT=$(HOME="$AUTO_HOME" \
    PATH="$AUTO_BIN:/usr/bin:/bin" \
    "$AUTO_BIN/tempo" wallet --version 2>&1)

echo "$AUTO_OUTPUT"
if echo "$AUTO_OUTPUT" | grep -q "Updated tempo-wallet"; then
    pass "auto-update: detected stale version and applied update"
else
    fail "auto-update: expected 'Updated tempo-wallet' in output, got: $AUTO_OUTPUT"
fi

# Verify state.json was updated
STATE_CONTENTS=$(cat "$AUTO_STATE_DIR/state.json")
if echo "$STATE_CONTENTS" | grep -q "$CURRENT_VERSION"; then
    pass "auto-update: state.json records $CURRENT_VERSION"
else
    fail "auto-update: expected $CURRENT_VERSION in state.json, got: $STATE_CONTENTS"
fi

# ------------------------------------------------------------------
step "All tests passed!"
echo ""
echo "Test home preserved at $TEST_DIR for inspection."
echo "Clean up with: rm -rf $TEST_DIR"
