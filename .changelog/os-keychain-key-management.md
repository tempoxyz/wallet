---
"presto": minor
---

Store wallet EOA private keys in the OS keychain (macOS Keychain / Linux Secret Service) instead of on disk. Access keys from `presto login` remain inline in `wallet.toml`. Add `presto key` subcommand for creating, importing, renaming, deleting, and switching between multiple named keys.
