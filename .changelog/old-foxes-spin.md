---
tempo-wallet: patch
tempo-common: patch
---

Migrated the `fund` command to a browser-based flow, replacing the previous faucet/bridge implementations with a simple browser-open + balance polling approach. Added network name aliases (`mainnet`, `testnet`, `moderato`) for `NetworkId` parsing, removed the `qrcode` dependency, updated install paths from `~/.local/bin` to `~/.tempo/bin`, and removed `--no-wait` and `--dry-run` flags from the fund command.
