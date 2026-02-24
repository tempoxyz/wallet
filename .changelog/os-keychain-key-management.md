---
"presto": minor
---

Store wallet EOA private keys in the OS keychain (macOS Keychain) instead of on disk. Access keys from ` tempo-walletlogin` remain inline in `keys.toml`. Add ` tempo-walletwallet` commands for creating, importing, and deleting local wallets.
