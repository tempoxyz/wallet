# Plan: Local Key Generation in CLI Auth Flow

## Problem

The browser currently generates the access key (secp256k1 keypair) during authentication and sends the **private key** to the CLI over the localhost callback. This means secret key material travels over HTTP between the browser and the local callback server, making it vulnerable to callback URL spoofing or local interception.

## Goal

Move key generation to the CLI so that only the **public key** is sent to the browser. The private key never leaves the CLI process. The browser uses the public key to generate a passkey-signed `key_authorization`, which is returned in the callback alongside the account address and metadata.

## Current Flow

```
 CLI                          Browser (app.tempo.xyz)           Chain
  │                              │                              │
  │  1. generate CSRF state      │                              │
  │  2. open browser ────────────>                              │
  │     /cli-auth                │                              │
  │     ?callback_url=           │                              │
  │      127.0.0.1:PORT/callback │                              │
  │     &state=CSRF_TOKEN        │                              │
  │                              │                              │
  │                   3. user taps passkey,                     │
  │                      browser generates secp256k1 keypair    │
  │                      and passkey signs key_authorization    │
  │                              │                              │
  │  4. POST /callback <─────────│                              │
  │     access_key (PRIV KEY) ⚠  │                              │
  │     account_address          │                              │
  │     key_id, expiry           │                              │
  │     key_authorization        │                              │
  │     state                    │                              │
  │                              │                              │
  │  5. validate CSRF state      │                              │
  │     save to wallet.toml      │                              │
  │                              │                              │
  ▼                              ▼                              ▼
```

## Proposed Flow

```
 CLI                          Browser (app.tempo.xyz)           Chain
  │                              │                              │
  │  1. generate secp256k1       │                              │
  │     keypair locally          │                              │
  │  2. generate CSRF state      │                              │
  │                              │                              │
  │  3. open browser ────────────>                              │
  │     /cli-auth                │                              │
  │     ?callback_url=           │                              │
  │      127.0.0.1:PORT/callback │                              │
  │     &state=CSRF_TOKEN        │                              │
  │     &pub_key=0xABC...        │                              │
  │                              │                              │
  │                   4. user taps passkey,                     │
  │                      passkey signs key_authorization        │
  │                      for pub_key (from URL param)           │
  │                              │                              │
  │  5. POST /callback <─────────│                              │
  │     account_address          │                              │
  │     key_id, expiry           │                              │
  │     key_authorization        │                              │
  │     state                    │                              │
  │                              │                              │
  │  6. validate CSRF state      │                              │
  │                              │                              │
  │  ── first payment ──────────────────────────────────────────│
  │                              │                              │
  │  7. on first 402 challenge,  │                              │
  │     include pending          │                              │
  │     key_authorization in tx  ──────────────────────────────>│
  │                              │                              │
  │                              │  8. chain registers key <────│
  │                              │     in Account Keychain      │
  │                              │                              │
  ▼                              ▼                              ▼
```

## Changes

### 1. Wallet Manager (`src/wallet/manager.rs`)

The core change. Both `setup_wallet` and `refresh_access_key` need to:

- Generate a `PrivateKeySigner` locally using `PrivateKeySigner::random()`
- Derive the public key / address and include it as a `&pub_key=` query param when opening the browser
- Pass the locally generated private key to `save_credentials` / `save_access_key` instead of reading it from the callback
- The `AccessKey` is constructed from the local private key, not from `callback.access_key`

### 2. Auth Callback (`src/wallet/auth_server.rs`)

- Remove `access_key` from `AuthCallback` and `CallbackForm` — the browser no longer sends a private key
- The callback now only returns: `account_address`, `key_id`, `expiry`, `key_authorization`, `state`

### 3. Access Key Construction (in `manager.rs`)

- `save_credentials` and `save_access_key` accept the locally generated `PrivateKeySigner` as a parameter
- The private key is hex-encoded from the signer and stored in the `AccessKey`
- Everything else (account_address, expiry, key_authorization) still comes from the callback

### 4. No Changes Required

- `src/wallet/access_key.rs` — struct and serialization unchanged
- `src/wallet/signer.rs` — loads from wallet.toml the same way
- `src/wallet/credentials.rs` — storage format unchanged
- `src/payment/` — payment flow unchanged, `pending_key_authorization` consumed the same way
- `src/cli/` — CLI commands call the same `WalletManager` API

### 5. Webapp Coordination (out of scope for this repo)

The Tempo webapp (`app.tempo.xyz/cli-auth`) needs to:

- Accept `pub_key` as a URL query parameter
- Use that public key (instead of a locally generated one) when creating the `key_authorization`
- Stop sending `access_key` (private key) in the callback POST
- The callback form should omit the `access_key` field entirely

## Testing

### Unit Tests

- `manager.rs`: verify the browser URL includes `&pub_key=` param
- `auth_server.rs`: callback form without `access_key` field still deserializes correctly
- Existing `credentials.rs` and `signer.rs` tests pass unchanged (storage format is the same)

### Integration Tests

- Existing tests in `tests/` should pass — wallet.toml schema is unchanged
- Update any test fixtures that include `access_key` in mock callback payloads

### Manual Smoke Test

1. `pget wallet connect` — browser URL should contain `&pub_key=0x...`
2. After auth, `wallet.toml` contains a valid private key that was never sent to the browser
3. First payment with `pending_key_authorization` works as before
4. `pget wallet refresh` — same flow with `&account=` pinning


