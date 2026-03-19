# Architecture

Tempo CLI is a multi-crate workspace providing a command-line HTTP client with built-in [MPP](https://mpp.dev) payment support, wallet identity management, and a release signing tool. The top-level `tempo` launcher lives in the main tempo repo (`tempo/crates/ext/`).

## Crate Layering

```
tempo-wallet (wallet identity/custody + sessions/services/transfer)
  └── tempo-common (shared library)
tempo-request (HTTP client + payment)
  └── tempo-common (shared library)
tempo-sign (release signing, standalone)
tempo-test (shared test infrastructure, dev-only)
```

`tempo-common` is the shared foundation. `tempo-wallet` and `tempo-request` are independent binaries that both depend on it. `tempo-sign` is a standalone build tool. `tempo-test` provides mock servers, fixture builders, and assertion helpers used by integration tests across crates.

## `tempo-common` — Shared Library

Dependency flows top-down; lower layers never import from higher ones.

```
src/
├── cli/                    — shared CLI infrastructure
│   ├── args.rs             — GlobalArgs, parse_cli
│   ├── context.rs          — Context struct (Config, NetworkId, Keystore, Analytics, OutputFormat, Verbosity)
│   ├── exit_codes.rs       — process exit codes (ExitCode enum)
│   ├── format.rs           — value formatting helpers (amounts, durations, timestamps)
│   ├── output.rs           — OutputFormat, structured output helpers
│   ├── runner.rs           — CLI lifecycle (run_cli, run_main)
│   ├── runtime.rs          — tracing setup, color mode, error rendering
│   ├── terminal.rs         — terminal output helpers (hyperlinks, field formatting, sanitization)
│   ├── tracking.rs         — analytics tracking (track_command, track_result)
│   └── verbosity.rs        — verbosity configuration
├── keys/                   — key storage, signing, authorization
│   ├── authorization.rs    — on-chain key authorization proofs
│   ├── io.rs               — key file I/O (read/write keys.toml)
│   ├── keystore.rs         — Keystore struct, key selection logic
│   ├── model.rs            — key data model (KeyEntry, WalletType, KeyType)
│   └── signer.rs           — signer resolution (EOA vs keychain)
├── payment/                — payment error classification and session management
│   ├── classify.rs         — payment error classification and extraction
│   └── session/            — channel persistence, queries, close, tx signing
│       ├── channel.rs      — on-chain channel queries (balance, state, grace period)
│       ├── close/
│       │   ├── cooperative.rs — cooperative (off-chain) channel close
│       │   └── onchain.rs    — payer-initiated on-chain requestClose → withdraw
│       ├── store/
│       │   ├── model.rs    — domain model (ChannelRecord, ChannelStatus, PendingClose)
│       │   └── storage.rs  — SQLite persistence (open, insert, update, query)
│       └── tx.rs           — Tempo transaction submission
├── analytics.rs            — opt-out telemetry (PostHog)
├── config.rs               — configuration file handling
├── error.rs                — error types (ConfigError, TempoError, PaymentError, etc.)
├── lib.rs                  — module declarations and tempo_home()
├── network.rs              — chain definitions (Tempo, Moderato), explorer config, RPC
└── security.rs             — security utilities (safe logging, sanitization, redaction)
```

## `tempo-wallet` — Wallet Binary

Wallet identity and custody operations: login, key management, sessions, services, and transfers.

```
src/
├── main.rs                 — entry point
├── args.rs                 — Cli struct (flattens GlobalArgs), Commands, SessionCommands, ServicesCommands
├── app.rs                  — build Context, dispatch commands, track analytics
├── analytics.rs            — wallet-specific analytics events and payloads
├── prompt.rs               — interactive prompt helpers
├── wallet/
│   ├── types.rs            — wallet account types (balances, spending limits)
│   ├── query.rs            — on-chain wallet queries
│   └── render.rs           — wallet info rendering
└── commands/
    ├── login.rs            — passkey authentication flow
    ├── logout.rs           — disconnect wallet
    ├── whoami.rs           — wallet status, balances, keys
    ├── keys.rs             — key listing with balance and spending limit queries
    ├── transfer.rs         — TIP-20 token transfers (amount, token, recipient, fee estimation)
    ├── auth.rs             — shared browser/authentication utilities
    ├── debug.rs            — debug info collection for support tickets
    ├── completions.rs      — shell completions
    ├── fund/               — fund command (browser-based faucet/bridge flow)
    ├── sessions/           — session management
    │   ├── list.rs         — list active sessions (local + orphaned on-chain)
    │   ├── close.rs        — close sessions (cooperative, on-chain, finalize)
    │   ├── sync.rs         — sync local state with on-chain
    │   ├── render.rs       — session table rendering
    │   └── util.rs         — shared session helpers
    └── services/           — MPP service directory
        ├── client.rs       — service directory API client
        ├── model.rs        — service data model
        └── render.rs       — service listing rendering
```

## `tempo-request` — HTTP Client Binary

HTTP client with built-in MPP payment support. Handles `402 Payment Required` challenges natively.

```
src/
├── main.rs                 — entry point
├── args.rs                 — Cli struct (flattens GlobalArgs), QueryArgs
├── app.rs                  — dispatch to request command
├── analytics.rs            — request-specific analytics events and payloads
├── query/                  — query command flow
│   ├── analytics.rs        — query analytics tracking
│   ├── challenge.rs        — 402 challenge detection and dispatch
│   ├── headers.rs          — request header construction
│   ├── output.rs           — response output formatting
│   ├── payload.rs          — request body handling (--json, --data, stdin)
│   ├── prepare.rs          — request preparation (URL, method, headers, body)
│   └── sse.rs              — Server-Sent Events streaming
├── http/                   — HTTP client and response handling
│   ├── client.rs           — reqwest client construction
│   ├── fmt.rs              — verbose HTTP formatting (-v output)
│   └── response.rs         — response wrapper (status, headers, body)
└── payment/                — payment flows
    ├── challenge.rs        — shared challenge parsing helpers
    ├── charge.rs           — one-shot on-chain charge payment
    ├── lock.rs             — per-origin file locking for channel operations
    ├── router.rs           — payment mode dispatch (charge vs session)
    ├── types.rs            — shared types (ResolvedChallenge, PaymentResult)
    └── session/            — session-based payment
        ├── flow.rs         — stage-driven session orchestration
        ├── open.rs         — channel opening and initial credential handshake
        ├── voucher.rs      — off-chain voucher signing and transport
        ├── streaming.rs    — SSE streaming with per-token voucher top-ups
        ├── persist.rs      — session persistence (save/update channel records)
        ├── receipt.rs       — session receipt validation
        └── error_map.rs    — HTTP rejection → PaymentRejected mapping
```

## `tempo-sign` — Release Signing Tool

Standalone tool for generating signed release manifests to authenticate build artifacts.

```
src/
├── main.rs                 — entry point
├── args.rs                 — CLI argument definitions (generate-key, sign, verify)
├── error.rs                — signing-specific error types
├── key.rs                  — minisign keypair generation and loading
├── manifest.rs             — release manifest construction
└── sign.rs                 — manifest signing and verification
```

## `tempo-test` — Test Infrastructure

Shared test infrastructure used by integration tests across crates. Not published.

```
src/
├── lib.rs                  — re-exports all modules
├── assert.rs               — assertion helpers for CLI output
├── command.rs              — test_command builder with proper config
├── fixture.rs              — TestConfigBuilder for test setup
└── mock.rs                 — mock HTTP/payment servers
```

## Payment Flows

### Charge (One-Shot)

Implemented in `tempo-request/src/payment/charge.rs`. Handles single-request on-chain settlement.

1. The server responds with HTTP 402 and a `WWW-Authenticate` header describing the payment terms.
2. The challenge is parsed via the `mpp` crate.
3. A signed transaction is built using `mpp::TempoProvider` and submitted on-chain.
4. The request is retried with an `Authorization` header containing the payment credential (transaction hash).

This mode requires no persistent state — each request is independently settled.

### Session (Channel)

Session orchestration is implemented in `tempo-request/src/payment/session/`. Shared session infrastructure (persistence, channel queries, close operations, tx signing) lives in `tempo-common/src/payment/session/`.

`handle_session_request` is stage-driven with explicit boundaries:

1. **Challenge stage** — parses/validates the challenge and resolves normalized session identity.
2. **Deposit stage** — derives deposit policy and wallet-balance clamp behavior.
3. **Reuse stage** — discovers/revalidates reusable channels (local plus on-chain identity checks).
4. **Open stage** — performs channel open and initial credential handshake.
5. **Request stage** — executes the paid request and receipt persistence.

Session invariants are intentionally strict:

- Session challenge `methodDetails.chainId` is required; missing `chainId` is rejected.
- Paid SSE requests fail closed on stream timeout/retry exhaustion/incomplete termination.
- Persisted channel `cumulative_amount` is monotonic and must never decrease.

Session HTTP rejection mapping is centralized in `error_map.rs` so `flow.rs`, `open.rs`, and `streaming.rs` share one sanitization and length-bounding policy for server-derived `PaymentRejected.reason` text.

#### Voucher Transport

1. Voucher updates are attempted with `HEAD` first.
2. Fallback to `POST` when `HEAD` is unsupported (`405`/`501`) or transport fails.
3. Voucher/top-up submissions use a dedicated reqwest client handle (separate from stream response reading) while preserving the same transport policy as the primary request client.

Streaming voucher retries are managed by an explicit coordinator in `streaming.rs` that owns pending-voucher state, retry counters, and stall-timeout backoff progression.

#### Channel Lifecycle

1. On first request, a channel is opened on-chain with a deposit.
2. Subsequent requests exchange off-chain vouchers — signed cumulative amounts — instead of on-chain transactions.
3. SSE streaming is supported: per-token voucher top-ups are issued as streamed data arrives.
4. Channel state persists across CLI invocations in a SQLite database (`channels.db`).
5. Channels can be closed explicitly. Local rows track explicit lifecycle state (active, closing, finalizable, finalized). Orphaned channels and close readiness are derived from on-chain state when needed.

#### Channel Close Timing

1. `requestClose()` starts the escrow grace window.
2. `withdraw()` is attempted when `now >= closeRequestedAt + gracePeriod`.
3. The CLI does not currently add an extra cushion beyond contract grace by default.

#### Receipt Policy

- Missing or invalid `Payment-Receipt` on otherwise successful paid responses emits warnings.
- Runtime requests are not failed solely for missing/invalid receipts.

## Typed Error Boundary Pattern

Error handling follows a typed-boundary model:

1. Prefer source-carrying variants (`*Source`) when an underlying error object exists.
2. Preserve user-facing wording stability at CLI boundaries by keeping display strings deterministic.
3. Reserve free-form string reasons for business-rule rejections where no concrete source error exists.

Compatibility exceptions are explicit and regression-tested:

- Payment classification keeps `NetworkError::Http(...)` as an opaque fallback for unmatched provider errors.
- Router network mismatch intentionally uses `PaymentError::ChallengeSchema` with the preserved wording: `Server requested network '...' but --network is '...'`.

## Wallet Types

### Passkey

Browser-based WebAuthn wallet created via Tempo's passkey flow (`login.rs`). Authentication is delegated to the browser; the wallet address and key authorization are stored locally.

### Local

Locally generated or imported secp256k1 private key. The private key is stored inline in a mode-0600 `keys.toml` file.

### Signing Modes

Determined by the relationship between `wallet_address` and `key_address` (`tempo-common/src/keys/signer.rs`):

- **Direct EOA signing** — when the wallet address equals the key address, transactions are signed directly.
- **Keychain (smart wallet) signing** — otherwise, transactions are signed with the authorized sub-key and include the on-chain key authorization proof.

Key selection is deterministic: passkey → first key with inline `key` → first key (lexicographic).

## Channel Persistence

- SQLite database stored at `$TEMPO_HOME/wallet/channels.db` (default: `~/.tempo/wallet/channels.db`).
- Keyed by `channel_id` with an origin index for reuse lookups.
- `ChannelRecord` stores channel state: channel ID, cumulative amount, deposit, payer/payee/token identity, and challenge echo data.
- No fixed TTL is enforced; channels have no implicit expiry in local persistence.
- Pending closes are tracked separately for grace-period finalization.
- Monotonic channel accounting is enforced at storage update boundaries (`update_channel_cumulative_floor`).

## `mpp` Boundary Guarantees

Protocol-critical behavior delegated to `mpp` is locked with local boundary tests so upstream changes cannot silently alter client conformance.

1. EIP-712 voucher signatures are verified as domain-bound to `chain_id` and `verifying_contract`.
2. Voucher verification is locked to canonical 65-byte signatures, and compact ERC-2098 signatures are normalized to canonical form at the local boundary before verification.
3. Unknown-field tolerance is verified for session request, credential payload, and receipt parsing.
4. RFC 9457 extension-field passthrough is verified in local problem parsing.

Boundary tests live in `crates/tempo-request/tests/mpp_boundary.rs`.

## Client Scope Boundaries

This repository is a client/reference wallet implementation. It enforces client-side requirements from the session spec and intentionally does not implement server-only operational MUSTs.

Server-side concerns explicitly out of scope include voucher rate limiting/anti-DoS policy, challenge-to-voucher audit trail persistence, receipt issuance guarantees, and per-session server accounting durability semantics.
