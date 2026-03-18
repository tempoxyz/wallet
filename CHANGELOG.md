# Changelog

## Unreleased

### Patch Changes

- Improve open-source readiness across docs and metadata: fix contributor setup instructions, replace dead example links, add a security policy document, and refresh the root README structure.
- Remove embedded routing/rate-limit tokens from built-in Tempo default RPC and wallet auth URLs, using token-free base endpoints instead.

## 0.1.4 (2026-03-18)

### Patch Changes

- Tighten charge payment provisioning retry to only fire on auth/payment (401–403) and server error (5xx) status codes, avoiding wasteful retries on unrelated API errors like 400 body validation. Show full server response body in payment rejection errors instead of extracting a single JSON field. Ensure all retry paths surface the original error on retry failure.

## 0.1.4 (2026-03-18)

## 0.1.3 (2026-03-18)

### Patch Changes

- Fix charge payment failing with "access key does not exist" when the signing key is not yet provisioned on-chain. The server-side rejection retry only triggered on HTTP 401-403, but the server returns other status codes for keychain errors.

## 0.1.3 (2026-03-18)

### Patch Changes

- Fix charge payment failing with "access key does not exist" when the signing key is not yet provisioned on-chain. The server-side rejection retry only triggered on HTTP 401-403, but the server returns other status codes for keychain errors.

## 0.1.2 (2026-03-18)

### Patch Changes

- Persist channel state when channel open fails to prevent orphaned on-chain funds.
- When the open transaction is sent to the server but the server returns an error, the channel may already exist on-chain. Previously the channel state was lost, making the deposited funds unrecoverable. Now the channel record is persisted before returning the error, so future runs can discover and close it.
- Fix session deposit and key-auth retry error handling.
- Preserve original error when key-authorization retry also fails, instead of showing misleading retry errors (e.g. `KeyAlreadyExists`).
- Ensure session deposit covers at least the per-request cost, preventing channels from opening with insufficient funds.
- Fail early before opening a channel if wallet balance is too low to cover the request cost.

## 0.1.2 (2026-03-18)

## 0.1.1 (2026-03-18)

### Patch Changes

- Removed the "all" amount option from the transfer command, along with the associated `query_balance` helper and `balanceOf` interface method. The `resolve_amount` function is now synchronous.
- Migrated the `fund` command to a browser-based flow, replacing the previous faucet/bridge implementations with a simple browser-open + balance polling approach. Added network name aliases (`mainnet`, `testnet`, `moderato`) for `NetworkId` parsing, removed the `qrcode` dependency, updated install paths from `~/.local/bin` to `~/.tempo/bin`, and removed `--no-wait` and `--dry-run` flags from the fund command.

## 0.1.1 (2026-03-17)

### Patch Changes

- Simplified optimistic key provisioning by removing the `query_key_status` and `prepare_provisioning_retry` functions, instead retrying directly with `with_key_authorization()` on any error. Added support for merged `WWW-Authenticate` challenges (RFC 9110 §11.6.1) by splitting and selecting the first supported payment method. Fixed `list_channels` to exclude localhost origins and removed the realm-vs-origin validation check.
