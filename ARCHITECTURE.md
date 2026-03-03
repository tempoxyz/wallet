# Architecture

`presto` is a CLI HTTP client with built-in [MPP](https://mpp.dev) payment support. It sends HTTP requests and, when a server responds with `402 Payment Required`, automatically negotiates and executes payment before retrying.

## Module Layering

Dependency flows top-down; lower layers never import from higher ones.

```
main.rs          — entry point; dispatches to cli
  cli/           — user-facing commands; depends on all lower layers
  payment/       — payment flows (charge + session); depends on wallet, config, network
  wallet/        — wallet credentials, signing, keychain; depends on config, network
  config.rs      — configuration file handling; depends on error
  network.rs     — chain definitions, explorer config, RPC; depends on error
  http.rs        — HTTP client wrapper; depends on config
  services/      — MPP service directory; depends on http
  analytics/     — opt-out telemetry; no internal dependencies
  error.rs       — error types; foundational
  util.rs        — shared utilities; foundational
```

## Payment Flows

### Charge (one-shot)

Implemented in `payment/charge.rs`. Handles single-request on-chain settlement.

1. The server responds with HTTP 402 and a `WWW-Authenticate` header describing the payment terms.
2. Presto parses the challenge via the `mpp` crate.
3. A signed transaction is built using `mpp::TempoProvider` and submitted on-chain.
4. The request is retried with an `Authorization` header containing the payment credential (transaction hash).

This mode requires no persistent state — each request is independently settled.

### Session (channel)

Implemented in `payment/session/`. Provides a persistent payment channel for repeated requests to the same origin.

1. On first request, presto opens an on-chain channel with a deposit.
2. Subsequent requests exchange off-chain vouchers — signed cumulative amounts — instead of on-chain transactions.
3. SSE streaming is supported: per-token voucher top-ups are issued as streamed data arrives.
4. Sessions persist across CLI invocations in a SQLite database (`payment/session/store.rs`).
5. Channels can be closed explicitly. Local rows track explicit lifecycle state (active, closing, finalizable). Orphaned channels and close readiness are derived from on-chain state when needed.

## Wallet Types

### Passkey

Browser-based WebAuthn wallet created via Tempo's passkey flow (`wallet/passkey.rs`). Authentication is delegated to the browser; presto stores the resulting wallet address and key authorization.

### Local

Locally generated or imported secp256k1 private key (`wallet/credentials/`). The private key is stored in the OS keychain on macOS (`wallet/keychain.rs`) or inline in a mode-0600 `keys.toml` file.

### Signing Modes

Determined by the relationship between `wallet_address` and `key_address` (`wallet/signer.rs`):

- **Direct EOA signing** — when the wallet address equals the key address, transactions are signed directly.
- **Keychain (smart wallet) signing** — otherwise, transactions are signed with the authorized sub-key and include the on-chain key authorization proof.

Key selection is deterministic: passkey > first key with inline `key` > first key (lexicographic).

## Session Persistence

- SQLite database stored in the platform data directory.
- Keyed by origin URL — returning requests to the same origin reuse existing channels.
- `SessionRecord` stores channel state: channel ID, cumulative amount, deposit, nonce, and signing material.
- 24-hour TTL on sessions; expired sessions are cleaned up automatically.
- Pending closes are tracked separately for grace-period finalization.

## Key Files

| Path | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point, module declarations |
| `src/http.rs` | `HttpClient` (reqwest wrapper), `RequestContext`, `RequestRuntime`, header/body helpers |
| `src/cli/args.rs` | Clap definitions (`Cli`, `QueryArgs`, `Commands`) |
| `src/cli/query.rs` | Primary query flow: HTTP → 402 detection → payment → retry |
| `src/cli/auth.rs` | Login, logout, whoami commands |
| `src/cli/keys.rs` | Key listing with balance and spending limit queries |
| `src/cli/session/` | Session management commands (list/info/close/recover/sync) |
| `src/cli/output.rs` | Response display formatting, `OutputOptions` |
| `src/payment/charge.rs` | One-shot on-chain charge payment |
| `src/payment/session/` | Session-based payment channel (open, voucher, close, store) |
| `src/wallet/signer.rs` | Signing mode selection and transaction signing |
| `src/wallet/keychain.rs` | macOS Keychain integration for private key storage |
| `src/wallet/credentials/` | Wallet credential management (create, import, delete) |
| `src/config.rs` | Config file parsing and RPC resolution |
| `src/network.rs` | Built-in network definitions (Tempo, Moderato), explorer URLs |
| `src/analytics/` | Opt-out PostHog telemetry |
| `src/cli/fund.rs` | Wallet funding: testnet faucet or mainnet bridge via Relay |
| `src/cli/relay.rs` | Relay bridge client for cross-chain wallet funding |
| `src/cli/services.rs` | Service directory listing and detail views |
| `src/services/` | MPP service registry fetching and data model |
| `src/error.rs` | `PrestoError` enum (thiserror) |
| `src/util.rs` | Atomic file writes, terminal hyperlinks |
