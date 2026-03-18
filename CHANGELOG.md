# Changelog

## 0.1.1 (2026-03-18)

### Patch Changes

- Removed the "all" amount option from the transfer command, along with the associated `query_balance` helper and `balanceOf` interface method. The `resolve_amount` function is now synchronous.
- Migrated the `fund` command to a browser-based flow, replacing the previous faucet/bridge implementations with a simple browser-open + balance polling approach. Added network name aliases (`mainnet`, `testnet`, `moderato`) for `NetworkId` parsing, removed the `qrcode` dependency, updated install paths from `~/.local/bin` to `~/.tempo/bin`, and removed `--no-wait` and `--dry-run` flags from the fund command.

## 0.1.1 (2026-03-17)

### Patch Changes

- Simplified optimistic key provisioning by removing the `query_key_status` and `prepare_provisioning_retry` functions, instead retrying directly with `with_key_authorization()` on any error. Added support for merged `WWW-Authenticate` challenges (RFC 9110 §11.6.1) by splitting and selecting the first supported payment method. Fixed `list_channels` to exclude localhost origins and removed the realm-vs-origin validation check.

