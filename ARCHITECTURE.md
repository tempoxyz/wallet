# Architecture

Tempo CLI is a multi-crate workspace providing a command-line HTTP client with built-in [MPP](https://mpp.dev) payment support, wallet identity management, and a release signing tool. The top-level `tempo` launcher lives in the main tempo repo (`tempo/crates/ext/`).

## Crate Layering

```
tempo-wallet (wallet identity/custody + sessions/services/sign)
  └── tempo-common (shared library)
tempo-request (HTTP client + payment)
  └── tempo-common (shared library)
tempo-sign (release signing, standalone)
```

`tempo-common` is the shared foundation. `tempo-wallet` and `tempo-request` are independent binaries that both depend on it. `tempo-sign` is a standalone build tool.

## tempo-common Module Layering

Dependency flows top-down; lower layers never import from higher ones.

```
tempo-common/src/
  cli/                 — shared CLI infrastructure (submodules below)
    args.rs            — GlobalArgs, parse_cli
    context.rs         — Context struct (Config, NetworkId, Keystore, Analytics, OutputFormat, Verbosity)
    exit_codes.rs      — process exit codes (ExitCode enum)
    format.rs          — value formatting helpers (amounts, durations, timestamps)
    output.rs          — OutputFormat, structured output helpers
    runner.rs          — CLI lifecycle (run_cli, run_main)
    runtime.rs         — tracing setup, color mode, error rendering
    terminal.rs        — terminal output helpers (hyperlinks, field formatting, sanitization)
    tracking.rs        — analytics tracking (track_command, track_result)
    verbosity.rs       — verbosity configuration
  config.rs            — configuration file handling; depends on error
  network.rs           — chain definitions, explorer config, RPC; depends on error
  error.rs             — error types (ConfigError, TempoError); foundational
  analytics.rs         — opt-out telemetry; no internal dependencies
  security.rs          — security utilities (safe logging, sanitization, redaction)
  keys/                — key storage, signing, authorization; depends on config, network
  payment/             — payment error classification and session management
    classify.rs        — payment error classification and extraction
    session/           — session persistence (store.rs), channel queries, close operations, tx signing
```

## Binary Crate Structure

### tempo-wallet

```
tempo-wallet/src/
  main.rs              — entry point
  args.rs              — Cli struct (flattens GlobalArgs from tempo_common::cli)
  app.rs               — build Context, dispatch commands, track analytics
  analytics.rs         — wallet-specific analytics events and payloads
  prompt.rs            — interactive prompt helpers
  wallet/              — wallet account types, on-chain queries, rendering
    types.rs, query.rs, render.rs
  commands/
    login.rs           — passkey authentication flow
    logout.rs          — disconnect wallet
    whoami.rs          — wallet status, balances, keys
    keys.rs            — key listing with balance and spending limit queries
    sign.rs            — sign MPP payment challenges
    completions.rs     — shell completions
    fund/              — fund subcommands (faucet, bridge, relay)
    sessions/          — session management (list, close, sync, render)
    services/          — service directory (client, model, render)
```

### tempo-request

```
tempo-request/src/
  main.rs              — entry point
  args.rs              — Cli struct (flattens GlobalArgs), QueryArgs
  app.rs               — dispatch to request command
  analytics.rs         — request-specific analytics events and payloads
  query/               — query command flow (request prep, output, SSE, analytics)
    analytics.rs, challenge.rs, headers.rs, output.rs, payload.rs, prepare.rs, sse.rs
  http/                — HTTP client, response handling, formatting
    client.rs, fmt.rs, response.rs
  payment/             — payment flows (charge + session)
    charge.rs, router.rs
    session/           — session flow, channel opening, voucher, streaming, persistence
```

## Typed Error Boundary Pattern

Error handling follows a typed-boundary model:

1. Prefer source-carrying variants (`*Source`) when an underlying error object exists.
2. Preserve user-facing wording stability at CLI boundaries by keeping display strings deterministic.
3. Reserve free-form string reasons for business-rule rejections where no concrete source error exists.

This means conversion at boundaries should look like:

- Parse/format/schema failures: wrap the concrete source error (`PaymentError::ChallengeParseSource`, `PaymentError::ChallengeFormatSource`, `NetworkError::ResponseSchemaSource`, etc.).
- Session persistence and reuse protection: keep causal source chains (`SessionPersistenceSource` / `SessionPersistenceContextSource`) so troubleshooting retains root cause fidelity.
- Business-rule denials (for example client-side `--max-pay` policy checks): use stable reason strings intentionally.

Compatibility exceptions are explicit and regression-tested:

- Payment classification keeps `NetworkError::Http(...)` as an opaque fallback for unmatched provider errors.
- Router network mismatch intentionally uses `PaymentError::ChallengeSchema` with the preserved wording: `Server requested network '...' but --network is '...'`.

## Payment Flows

### Charge (one-shot)

Implemented in `tempo-request/src/payment/charge.rs`. Handles single-request on-chain settlement.

1. The server responds with HTTP 402 and a `WWW-Authenticate` header describing the payment terms.
2. The challenge is parsed via the `mpp` crate.
3. A signed transaction is built using `mpp::TempoProvider` and submitted on-chain.
4. The request is retried with an `Authorization` header containing the payment credential (transaction hash).

This mode requires no persistent state — each request is independently settled.

### Session (channel)

Session orchestration (flow, streaming, voucher) is implemented in `tempo-request/src/payment/session/`. Shared session infrastructure (persistence, channel queries, close operations, tx signing) lives in `tempo-common/src/payment/session/`.

1. On first request, a channel is opened on-chain with a deposit.
2. Subsequent requests exchange off-chain vouchers — signed cumulative amounts — instead of on-chain transactions.
3. SSE streaming is supported: per-token voucher top-ups are issued as streamed data arrives.
4. Sessions persist across CLI invocations in a SQLite database (`tempo-common/src/payment/session/store.rs`).
5. Channels can be closed explicitly. Local rows track explicit lifecycle state (active, closing, finalizable). Orphaned channels and close readiness are derived from on-chain state when needed.

## Wallet Types

### Passkey

Browser-based WebAuthn wallet created via Tempo's passkey flow (`tempo-wallet/src/commands/login.rs`). Authentication is delegated to the browser; the wallet address and key authorization are stored locally.

### Local

Locally generated or imported secp256k1 private key. The private key is stored inline in a mode-0600 `keys.toml` file.

### Signing Modes

Determined by the relationship between `wallet_address` and `key_address` (`tempo-common/src/keys/signer.rs`):

- **Direct EOA signing** — when the wallet address equals the key address, transactions are signed directly.
- **Keychain (smart wallet) signing** — otherwise, transactions are signed with the authorized sub-key and include the on-chain key authorization proof.

Key selection is deterministic: passkey > first key with inline `key` > first key (lexicographic).

## Session Persistence

- SQLite database stored at `$TEMPO_HOME/wallet/sessions.db` (default: `~/.tempo/wallet/sessions.db`).
- Keyed by origin URL — returning requests to the same origin reuse existing channels.
- `SessionRecord` stores channel state: channel ID, cumulative amount, deposit, nonce, and signing material.
- 24-hour TTL on sessions; expired sessions are cleaned up automatically.
- Pending closes are tracked separately for grace-period finalization.

## Key Files

| Path | Purpose |
|------|---------|
| `crates/tempo-common/src/cli/` | Shared CLI infrastructure (args, context, output, runner, runtime, tracking) |
| `crates/tempo-common/src/error.rs` | Error types: `ConfigError`, `TempoError` (thiserror) |
| `crates/tempo-common/src/config.rs` | Config file parsing and RPC resolution |
| `crates/tempo-common/src/network.rs` | Built-in network definitions (Tempo, Moderato), explorer URLs |
| `crates/tempo-common/src/analytics.rs` | Opt-out PostHog telemetry |
| `crates/tempo-common/src/security.rs` | Security utilities (safe logging, sanitization, redaction) |
| `crates/tempo-common/src/keys/` | Key storage (model, I/O), signer resolution, authorization |
| `crates/tempo-common/src/payment/classify.rs` | Payment error classification and extraction |
| `crates/tempo-common/src/payment/session/` | Session persistence (store), channel queries, close, tx signing |
| `crates/tempo-wallet/src/app.rs` | Wallet command dispatch lifecycle |
| `crates/tempo-wallet/src/wallet/` | Wallet account types (balances, spending limits), on-chain queries |
| `crates/tempo-wallet/src/commands/login.rs` | Login command and passkey authentication flow |
| `crates/tempo-wallet/src/commands/sessions/` | Session management commands (list/close/sync) |
| `crates/tempo-wallet/src/commands/services/` | Service directory listing and detail views |
| `crates/tempo-request/src/http/` | HTTP client, response handling, formatting |
| `crates/tempo-request/src/query/` | Query flow (challenge parsing, request prep, output, SSE, analytics) |
| `crates/tempo-request/src/payment/charge.rs` | One-shot on-chain charge payment |
| `crates/tempo-request/src/payment/session/` | Session flow, channel opening, voucher, streaming |
