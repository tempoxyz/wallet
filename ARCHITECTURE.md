# Architecture

`tempo-wallet` is a CLI HTTP client with built-in [MPP](https://mpp.dev) payment support. It sends HTTP requests and, when a server responds with `402 Payment Required`, automatically negotiates and executes payment before retrying.

## Module Layering

Dependency flows top-down; lower layers never import from higher ones.

```
main.rs            — entry point; parse CLI, run, handle errors
  cli/run.rs       — application lifecycle: tracing, color, context, dispatch, analytics
  cli/context.rs   — Context struct (Cli, Config, NetworkId, Keystore, Analytics, OutputFormat)
  cli/commands/    — command implementations; all take &Context as first arg
  account/         — wallet account types (balances, spending limits) and on-chain queries
  payment/         — payment flows (charge + session); depends on keys, config, network
  keys/            — key storage, signing, authorization; depends on config, network
  config.rs        — configuration file handling; depends on error
  network.rs       — chain definitions, explorer config, RPC; depends on error
  http/            — HTTP client wrapper; depends on network
  analytics.rs     — opt-out telemetry; no internal dependencies
  error.rs         — error types; foundational
  util.rs          — shared utilities; depends on network (for token formatting)
```

## Payment Flows

### Charge (one-shot)

Implemented in `payment/charge.rs`. Handles single-request on-chain settlement.

1. The server responds with HTTP 402 and a `WWW-Authenticate` header describing the payment terms.
2. Tempo Wallet parses the challenge via the `mpp` crate.
3. A signed transaction is built using `mpp::TempoProvider` and submitted on-chain.
4. The request is retried with an `Authorization` header containing the payment credential (transaction hash).

This mode requires no persistent state — each request is independently settled.

### Session (channel)

Implemented in `payment/session/`. Provides a persistent payment channel for repeated requests to the same origin.

1. On first request, tempo-wallet opens an on-chain channel with a deposit.
2. Subsequent requests exchange off-chain vouchers — signed cumulative amounts — instead of on-chain transactions.
3. SSE streaming is supported: per-token voucher top-ups are issued as streamed data arrives.
4. Sessions persist across CLI invocations in a SQLite database (`payment/session/store.rs`).
5. Channels can be closed explicitly. Local rows track explicit lifecycle state (active, closing, finalizable). Orphaned channels and close readiness are derived from on-chain state when needed.

## Wallet Types

### Passkey

Browser-based WebAuthn wallet created via Tempo's passkey flow (`cli/commands/login.rs`). Authentication is delegated to the browser; tempo-wallet stores the resulting wallet address and key authorization.

### Local

Locally generated or imported secp256k1 private key (`cli/commands/wallets/`). The private key is stored in the OS keychain on macOS (`keys/keychain.rs`) or inline in a mode-0600 `keys.toml` file.

### Signing Modes

Determined by the relationship between `wallet_address` and `key_address` (`keys/signer.rs`):

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
| `src/cli/run.rs` | Application lifecycle: init, build `Context`, dispatch commands, track analytics |
| `src/cli/context.rs` | `Context` struct: shared app state threaded to all commands |
| `src/cli/args.rs` | Clap definitions (`Cli`, `QueryArgs`, `Commands`) |
| `src/cli/output.rs` | `OutputFormat`, `OutputOptions` |
| `src/account/` | Wallet account types (balances, spending limits), on-chain queries |
| `src/cli/commands/query/` | Primary query flow: HTTP → 402 detection → payment → retry |
| `src/cli/commands/login.rs` | Login command and passkey authentication flow |
| `src/cli/commands/logout.rs` | Logout command |
| `src/cli/commands/whoami.rs` | Whoami command |
| `src/cli/commands/keys.rs` | Key listing with balance and spending limit queries |
| `src/cli/commands/sessions/` | Session management commands (list/info/close/sync) |
| `src/cli/commands/wallets/` | Wallet management (create, renew, list, fund) |
| `src/cli/commands/services/` | Service directory listing and detail views |
| `src/http/` | `HttpClient` (reqwest wrapper with retry logic), `HttpRequestPlan`, header/body helpers |
| `src/keys/` | Key storage (model, I/O), signer resolution, authorization |
| `src/payment/charge.rs` | One-shot on-chain charge payment |
| `src/payment/session/` | Session-based payment channel (open, voucher, close, store) |
| `src/config.rs` | Config file parsing and RPC resolution |
| `src/network.rs` | Built-in network definitions (Tempo, Moderato), explorer URLs |
| `src/analytics.rs` | Opt-out PostHog telemetry |
| `src/error.rs` | `TempoWalletError` enum (thiserror) |
| `src/util.rs` | Formatting helpers, terminal hyperlinks |
