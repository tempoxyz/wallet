# Wallet / Key Redesign Spec

## Overview

Decouple wallets and keys. Wallets are on-chain accounts users fund. Keys are access keys (signers on the AA wallet) and the primary primitive users interact with. Two wallet creation paths: passkey (browser auth) and local (self-custodial EOA).

## CLI

### `presto wallet create [--name NAME]`

Create a local EOA wallet with an access key.

1. Generate random EOA key → store in OS keychain (wallet owner key)
2. Generate random access key → store inline in keys.toml
3. Generate a key_authorization signed by the local EOA for chain_id = 0 (applies to all chains) → store inline in keys.toml
4. Do not provision; the access key will auto-provision on first successful payment using the stored key_authorization
5. Print the fundable wallet address

Default `--name` is `"default"`.

### `presto wallet create [--name NAME] --passkey`

Create a passkey-based wallet via browser auth flow (current `presto login` behavior).

Default `--name` is `"passkey"`.

If a passkey key already exists:
```
Already logged in as 0xCCC...
To switch wallets, run 'presto logout' first.
```

### `presto wallet delete <name> [--yes]`

Remove a wallet (any type) by name. Deletes the key entry from keys.toml and the keychain secret (if local).

If deleting the active key:
- If other keys remain: auto-switch to the first remaining key (sorted) and print "Switched active key to '<name>'."
- If no keys remain: clear `active` and print "No wallets configured."

### `presto wallet delete --passkey [--yes]`

Remove the passkey wallet. Finds the key entry with `wallet_type = "passkey"` and removes it. Same auto-switch behavior as above.

### `presto login`

Alias for `presto wallet create --passkey`.

### `presto logout [--yes]`

Alias for `presto wallet delete --passkey`.

If no passkey wallet exists: no-op, prints "Not logged in." and exits 0.

## Data Model

### File: `keys.toml`

Replaces `wallet.toml`. Located in the same data directory.

```toml
active = "passkey"

[keys.default]
wallet_type = "local"
wallet_address = "0xAAA..."
access_key_address = "0xBBB..."
access_key = "0x..."
key_authorization = "0x..."
provisioned_chain_ids = [4217]

[keys.passkey]
wallet_type = "passkey"
wallet_address = "0xCCC..."
access_key_address = "0xDDD..."
access_key = "0x..."
key_authorization = "0x..."
provisioned_chain_ids = [4217]
```

### Fields

| Field | Description |
|-------|-------------|
| `active` | Name of the active key |
| `wallet_type` | `"local"` (EOA in keychain) or `"passkey"` (browser auth) |
| `wallet_address` | On-chain wallet address (the fundable address) |
| `access_key_address` | Public address of the access key |
| `access_key` | Access key private key (inline, wrapped in `Zeroizing`) |
| `key_authorization` | On-chain authorization proof |
| `provisioned_chain_ids` | Chains this key is provisioned on |

Notes:
- `key_authorization` uses `chain_id = 0` to indicate it applies to all chains (supported by on-chain contracts).
- `access_key` is plaintext secret material; file is written with mode 0600.

### Removed fields

| Field | Reason |
|-------|--------|
| `wallet_key_address` | Redundant — same as `wallet_address` for local wallets |
| `account_address` | Renamed to `wallet_address` |

## Active Key Resolution

When no `--key` flag is provided, the `active` field in keys.toml determines the active key.

If `active` is empty or unset and keys exist, the passkey entry is preferred if present; otherwise the first key (sorted by name) is used.

## Signer Resolution

1. `--private-key` override → use it directly
2. Active key's inline `access_key` → use it

No fallback to the OS keychain for payment signing. If the active key does not have an inline `access_key`, signing fails.

The local EOA stored in the OS keychain is used only at create time to sign the initial `key_authorization`.

## State Invariants

- Multiple key entries may coexist (local + passkey).
- At most one entry with `wallet_type = "passkey"` may exist.
- No limit on `wallet_type = "local"` entries.
- Key names must be unique within keys.toml.
- `active` points to exactly one key name, or is empty when no keys exist.
- Keychain deletion is best-effort (warn on failure, proceed with keys.toml removal).

## Code Changes

| Current | New |
|---------|-----|
| `wallet.toml` | `keys.toml` |
| `WALLET_FILE_NAME` | `KEYS_FILE_NAME` |
| `Key` struct | `KeyEntry` struct (or similar) |
| `account_address` field | `wallet_address` field |
| `wallet_key_address` field | removed |
| (new) | `wallet_type` field |
| `cli/key.rs` | `cli/wallet.rs` |
| `KeyCommands` | `WalletCommands { Create, Delete }` |
| `Commands::Key` | `Commands::Wallet` |
| `Commands::Login` | alias → `wallet create --passkey` |
| `Commands::Logout` | alias → `wallet delete --passkey` |

## Behavior

### Guards

- `presto wallet create --passkey`: rejects if a `wallet_type = "passkey"` entry already exists.
- `presto wallet create`: rejects if the target `--name` already exists (use a different name).
- Both: do not enforce mutual exclusion between local and passkey — both can coexist.

### `presto logout` only affects passkey wallets

`presto logout` finds and removes the `wallet_type = "passkey"` entry. It does not touch local wallets. Use `presto wallet delete <name>` for local wallets.

If no passkey entry exists: no-op, prints "Not logged in." and exits 0.

### `--key` flag

Selects the active key by name. Unchanged.

### `--private-key` flag

Ephemeral override. Unchanged.

## Unchanged

- `presto whoami` / `presto balance`
- `presto session` commands
- `presto completions`
- `--private-key`, `--key`, `--network`, `--config` flags
- Keychain backend (`KeychainBackend` trait, `OsKeychain`, `InMemoryKeychain`)
- Payment flow, MPP protocol handling
- Analytics

## Defaults

- Default network: `tempo` when not explicitly specified.
- Default key name for local wallets: `"default"`.
- Default key name for passkey wallets: `"passkey"`.

## Future Work

- Recovery flow for local wallets: re-derive access key + key_authorization from the OS keychain EOA if `access_key` is lost.
- `presto wallet list` — show all wallets if multiple exist.
