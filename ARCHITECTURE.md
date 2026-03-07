# Architecture

Tempo CLI is a multi-crate workspace providing a command-line HTTP client with built-in [MPP](https://mpp.dev) payment support, wallet identity management, and a launcher for extension dispatch.

## Crate Layering

```
tempo-cli (launcher)
  ├── tempo-wallet (wallet identity/custody)
  │     └── tempo-common (shared library)
  └── tempo-mpp (HTTP client + payment)
        └── tempo-common (shared library)
```

`tempo-common` is the shared foundation. `tempo-wallet` and `tempo-mpp` are independent binaries that both depend on it. `tempo-cli` dispatches to them. `tempo-sign` is a standalone build tool.

## tempo-common Module Layering

Dependency flows top-down; lower layers never import from higher ones.

```
tempo-common/src/
  cli.rs               — GlobalArgs, dispatch tracking (track_command, track_result), run_main
  context.rs           — Context struct (Config, NetworkId, Keystore, Analytics, OutputFormat)
  runtime.rs           — tracing setup, color mode, error rendering
  output.rs            — OutputFormat, structured output helpers
  exit_codes.rs        — process exit codes (ExitCode enum)
  account/             — wallet account types (balances, spending limits) and on-chain queries
  payment/             — payment flows (charge + session); depends on keys, config, network
  keys/                — key storage, signing, authorization; depends on config, network
  config.rs            — configuration file handling; depends on error
  network.rs           — chain definitions, explorer config, RPC; depends on error
  http/                — HTTP client wrapper; depends on network
  analytics.rs         — opt-out telemetry; no internal dependencies
  error.rs             — TempoError enum; foundational
  util.rs              — shared utilities; depends on network (for token formatting)
```

## Binary Crate Structure

### tempo-wallet

```
tempo-wallet/src/
  main.rs              — entry point; calls tempo_common::cli::run_main()
  cli/
    args.rs            — Cli struct (flattens GlobalArgs from tempo_common::cli)
    dispatch.rs        — build Context, dispatch commands, track analytics
    commands/
      login.rs         — passkey authentication flow
      logout.rs        — disconnect wallet
      whoami.rs         — wallet status, balances, keys
      keys.rs          — key listing with balance and spending limit queries
      wallets/         — wallet management (create, list, fund)
      completions.rs   — shell completions
```

### tempo-mpp

```
tempo-mpp/src/
  main.rs              — entry point; calls tempo_common::cli::run_main()
  cli/
    args.rs            — Cli struct (flattens GlobalArgs, QueryArgs)
    dispatch.rs        — build Context, dispatch commands, track analytics
    output.rs          — OutputOptions, query-specific output types
    commands/
      query/           — HTTP query flow (request → 402 → payment → response)
      sessions/        — session management (list, info, close, recover, sync)
      services/        — service directory listing and details
      completions.rs   — shell completions
```

## Payment Flows

### Charge (one-shot)

Implemented in `tempo-common/src/payment/charge.rs`. Handles single-request on-chain settlement.

1. The server responds with HTTP 402 and a `WWW-Authenticate` header describing the payment terms.
2. The challenge is parsed via the `mpp` crate.
3. A signed transaction is built using `mpp::TempoProvider` and submitted on-chain.
4. The request is retried with an `Authorization` header containing the payment credential (transaction hash).

This mode requires no persistent state — each request is independently settled.

### Session (channel)

Implemented in `tempo-common/src/payment/session/`. Provides a persistent payment channel for repeated requests to the same origin.

1. On first request, a channel is opened on-chain with a deposit.
2. Subsequent requests exchange off-chain vouchers — signed cumulative amounts — instead of on-chain transactions.
3. SSE streaming is supported: per-token voucher top-ups are issued as streamed data arrives.
4. Sessions persist across CLI invocations in a SQLite database (`payment/session/store.rs`).
5. Channels can be closed explicitly. Local rows track explicit lifecycle state (active, closing, finalizable). Orphaned channels and close readiness are derived from on-chain state when needed.

## Wallet Types

### Passkey

Browser-based WebAuthn wallet created via Tempo's passkey flow (`tempo-wallet/src/cli/commands/login.rs`). Authentication is delegated to the browser; the wallet address and key authorization are stored locally.

### Local

Locally generated or imported secp256k1 private key (`tempo-wallet/src/cli/commands/wallets/`). The private key is stored in the OS keychain on macOS (`keys/keychain.rs`) or inline in a mode-0600 `keys.toml` file.

### Signing Modes

Determined by the relationship between `wallet_address` and `key_address` (`tempo-common/src/keys/signer.rs`):

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
| `crates/tempo-common/src/cli.rs` | GlobalArgs, dispatch tracking, run_main |
| `crates/tempo-common/src/context.rs` | Context struct: shared app state threaded to all commands |
| `crates/tempo-common/src/error.rs` | `TempoError` enum (thiserror) |
| `crates/tempo-common/src/output.rs` | OutputFormat, structured output helpers |
| `crates/tempo-common/src/account/` | Wallet account types (balances, spending limits), on-chain queries |
| `crates/tempo-common/src/http/` | HttpClient (reqwest wrapper with retry logic), HttpRequestPlan |
| `crates/tempo-common/src/keys/` | Key storage (model, I/O), signer resolution, authorization |
| `crates/tempo-common/src/payment/charge.rs` | One-shot on-chain charge payment |
| `crates/tempo-common/src/payment/session/` | Session-based payment channel (open, voucher, close, store) |
| `crates/tempo-common/src/config.rs` | Config file parsing and RPC resolution |
| `crates/tempo-common/src/network.rs` | Built-in network definitions (Tempo, Moderato), explorer URLs |
| `crates/tempo-common/src/analytics.rs` | Opt-out PostHog telemetry |
| `crates/tempo-wallet/src/cli/dispatch.rs` | Wallet command dispatch lifecycle |
| `crates/tempo-mpp/src/cli/dispatch.rs` | MPP command dispatch lifecycle |
| `crates/tempo-mpp/src/cli/commands/query/` | Primary query flow: HTTP → 402 detection → payment → retry |
| `crates/tempo-mpp/src/cli/commands/sessions/` | Session management commands (list/info/close/sync) |
| `crates/tempo-mpp/src/cli/commands/services/` | Service directory listing and detail views |
| `crates/tempo-wallet/src/cli/commands/login.rs` | Login command and passkey authentication flow |
