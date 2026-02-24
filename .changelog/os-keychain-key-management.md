---
"presto": minor
---

Store wallet EOA private keys in the OS keychain (macOS Keychain) instead of on disk. Access keys from `presto login` remain inline in `keys.toml`. Add `presto wallet` commands for creating, importing, and deleting local wallets.
