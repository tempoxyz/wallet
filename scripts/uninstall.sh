#!/usr/bin/env bash
set -euo pipefail

BOLD="\033[1m"
DIM="\033[2m"
GREEN="\033[32m"
RED="\033[31m"
RESET="\033[0m"

if [[ ! -t 1 ]]; then
  BOLD="" DIM="" GREEN="" RED="" RESET=""
fi

ok()   { echo -e "  ${GREEN}✓${RESET} $*"; }
skip() { echo -e "  ${DIM}– $*${RESET}"; }
fail() { echo -e "  ${RED}✗${RESET} $*"; }

echo ""
echo -e "${BOLD}Tempo Uninstaller${RESET}"
echo ""

# ── Remove binaries ──────────────────────────────────────────────────
echo -e "${BOLD}Removing binaries...${RESET}"
for dir in ~/.tempo/bin ~/.cargo/bin ~/.local/bin /usr/local/bin; do
  for bin in tempo tempo-wallet tempo-request tempo-core tempo-sign presto; do
    if [[ -f "$dir/$bin" ]]; then
      if rm -f "$dir/$bin" 2>/dev/null || sudo rm -f "$dir/$bin" 2>/dev/null; then
        ok "Removed $dir/$bin"
      else
        fail "Failed to remove $dir/$bin (try running with sudo)"
      fi
    fi
  done
done

# ── Remove config and data ───────────────────────────────────────────
echo ""
echo -e "${BOLD}Removing config and data...${RESET}"
for dir in \
  ~/Library/Application\ Support/tempo \
  ~/.config/tempo \
  ~/.local/share/tempo; do
  if [[ -d "$dir" ]]; then
    if rm -rf "$dir" 2>/dev/null || sudo rm -rf "$dir" 2>/dev/null; then
      ok "Removed $dir"
    else
      fail "Failed to remove $dir (try running with sudo)"
    fi
  else
    skip "$dir (not found)"
  fi
done

# ── Remove agent skills ─────────────────────────────────────────────
echo ""
echo -e "${BOLD}Removing agent skills...${RESET}"
found_skills=0
for dir in \
  ~/.agents/skills \
  ~/.claude/skills \
  ~/.config/agents/skills \
  ~/.cursor/skills \
  ~/.copilot/skills \
  ~/.codex/skills \
  ~/.gemini/skills \
  ~/.config/opencode/skills \
  ~/.config/goose/skills \
  ~/.windsurf/skills \
  ~/.codeium/windsurf/skills \
  ~/.continue/skills \
  ~/.roo/skills \
  ~/.kiro/skills \
  ~/.augment/skills \
  ~/.trae/skills; do
  for skill in tempo-wallet tempo-request tempo presto; do
    if [[ -d "$dir/$skill" ]]; then
      rm -rf "$dir/$skill"
      ok "Removed $dir/$skill"
      found_skills=1
    fi
  done
done
if [[ $found_skills -eq 0 ]]; then
  skip "No agent skills found"
fi

# ── Verify ───────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}Verifying...${RESET}"
all_clear=1
for bin in tempo tempo-wallet tempo-request tempo-core tempo-sign presto; do
  loc=$(which "$bin" 2>/dev/null || true)
  if [[ -n "$loc" ]]; then
    fail "$bin still found at $loc"
    all_clear=0
  else
    ok "$bin not found"
  fi
done

echo ""
if [[ $all_clear -eq 1 ]]; then
  echo -e "  ${GREEN}${BOLD}All clear!${RESET} Tempo has been fully removed."
else
  echo -e "  ${RED}${BOLD}Some binaries remain.${RESET} Remove them manually using the paths above."
fi
echo ""
