---
presto: minor
---

Added ` tempo-walletwallet fund` command with QR code display and Relay bridge integration for funding wallets from Base, Ethereum, Arbitrum, or Optimism via USDC. Added separate `presto-local` and `presto-passkey` skill variants with install script support for `--wallet=` flag, moved wallet balance to top-level in `whoami` JSON output, improved `InsufficientBalance` error messages with human-readable amounts, and removed automatic browser login prompts in favor of explicit wallet setup instructions.

Code quality improvements: extracted timeout and polling constants, fixed balance comparison to use numeric equality instead of string matching, added generic `poll_until` helper, added relay status constants, removed unused parameters, and added unit tests for balance change detection and relay deserialization. Removed `--passkey` flag from `wallet create` to keep passkey flow isolated to ` tempo-walletlogin`/` tempo-walletlogout`. Defaulted key authorization to USDC.e only on Tempo mainnet. Refactored install script: renamed `--local` to `--from-source`, removed redundant `--force` and `--passkey` flags, deduplicated agent directory lists and install logic. Removed emoji from CLI output.
