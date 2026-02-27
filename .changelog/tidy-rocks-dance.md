---
presto: minor
---

Added `presto wallet fund` command with QR code display and Relay bridge integration for funding wallets from Base, Ethereum, Arbitrum, or Optimism via USDC. Added separate `presto-local` and `presto-passkey` skill variants with install script support for `--wallet=` flag, moved wallet balance to top-level in `whoami` JSON output, improved `InsufficientBalance` error messages with human-readable amounts, and removed automatic browser login prompts in favor of explicit wallet setup instructions.
