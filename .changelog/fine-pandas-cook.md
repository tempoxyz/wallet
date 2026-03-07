---
tempo-common: minor
tempo-mpp: minor
tempo-wallet: minor
tempo-sign: patch
tempo-cli: patch
---

Split shared functionality into a new `tempo-common` crate and introduced a new `tempo-mpp` binary for HTTP query, sessions, and services commands. Renamed `sign-release` to `tempo-sign` and migrated all crate dependencies to workspace-level definitions. Refactored `tempo-wallet` to focus solely on wallet identity and custody operations (login, logout, whoami, keys, wallets).
