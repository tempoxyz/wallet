# Changelog

## 0.7.0 (2026-03-05)

### Minor Changes

- Refactored session management to consolidate close state into session records, replacing the separate `pending_closes` table with explicit `state`, `close_requested_at`, `grace_ready_at`, and `token_decimals` fields on `SessionRecord`. Added per-origin file locking (`fs2`) to prevent duplicate channel opens across processes, improved cooperative close to fetch a fresh challenge echo before submitting, and introduced "closing"/"finalizable" session statuses. Also fixed deposit scaling to respect token decimals and corrected the `whoami` ready flag to require a wallet.

### Patch Changes

- Added `sessions sync` command to reconcile local session records with on-chain state. Added locked/available balance breakdown to `whoami` and `keys` output showing funds held in active payment channels. Improved session close output messages with cleaner formatting and settlement transaction URLs. Removed session TTL expiry logic and migrated the sessions database schema to drop the `expires_at` column. Added `created_at` and `last_used_at` timestamps to session list output.
- Refactored payment dispatch to centralize challenge parsing and network/signer resolution in `dispatch_payment`, passing a `ResolvedChallenge` struct to `handle_charge_request` and `handle_session_request`. Moved `parse_payment_rejection` to `charge.rs`, removed the `SessionResult` enum in favor of `PaymentResult`, and eliminated redundant `Config` and network lookups from individual payment handlers.
- Refactored internal module structure by splitting `src/util.rs` into focused submodules (`util/format.rs`, `util/fs.rs`, `util/terminal.rs`), extracting `dispatch_payment` and `parse_payment_rejection` into `src/payment/dispatch.rs`, and moving CLI helpers (`init_tracing`, `generate_completions`, `print_version_json`) into `src/cli/logging.rs` and `src/cli/completions.rs`. Also moved retry logic from `query.rs` into `HttpRequestPlan` via a new `RetryPolicy` struct.

## 0.6.2 (2026-03-03)

### Patch Changes

- Added automatic update checking that fetches the latest version from the release CDN at most once every 6 hours and prints an upgrade notice if a newer version is available. Refactored config loading to happen once at startup and pass it through the call stack, removing redundant `load_config_with_overrides` calls throughout command handlers. Also removed the natural-language prompt forwarding to the `claude` CLI.
- Updated `mpp-rs` and `tempo` dependency revisions, fixed verbose logging filters to properly scope log levels per crate, updated passkey auth URLs to use `/cli-auth` path, and added `KeychainVersion::V1` to keychain signing mode.
- Fixed incorrect `presto session` command references to `presto sessions` in finalize hints and documentation comments.

## 0.6.1 (2026-03-02)

### Patch Changes

- Updated the install script to use `~/.local/bin` as the default installation directory instead of `/usr/local/bin`, removing the need for sudo. Added automatic PATH configuration for bash and zsh shell rc files, and added cleanup of legacy binaries from the old install location. Also updated passkey auth URLs for the tempo and moderato networks.
- Added TOON format support as a compact, token-efficient output and input option. Introduced `-t`/`--toon-output` flag for TOON-formatted output (recommended for agents) and `--toon <TOON>` option to send TOON-encoded request bodies decoded to JSON. Updated agent skill documentation to prefer `-t` over `-j` for token efficiency.
- Extended clickable terminal hyperlinks to wallet and key addresses displayed in the `auth`, `fund`, and `keys` CLI commands, so addresses in `wallet list`, `whoami`, `faucet`, and bridge deposit output are now rendered as clickable links pointing to the appropriate block explorer.
- Optimized request path to reuse a single HTTP client across initial and payment replay requests, enabling TCP/TLS connection pooling and skipping redundant TLS handshakes. Replaced on-chain nonce fetches with expiring nonces (`nonceKey=MAX`), removed the `find_channel_on_chain` recovery path, simplified the keychain/direct-signing flow to a unified path, and scoped verbose logging to the `presto` crate only.

## 0.6.0 (2026-02-27)

### Minor Changes

- Added `presto services` command for browsing the MPP service directory, supporting listing, filtering by category, searching by query, and viewing detailed service info including endpoints and pricing. Updated agent skill documentation to use pluralized subcommands (`wallets`, `keys`, `sessions`) and replace hardcoded service examples with generic `presto services`-based discovery patterns.
- Added `presto wallet fund` command with QR code display and Relay bridge integration for funding wallets from Base, Ethereum, Arbitrum, or Optimism via USDC. Added separate `presto-local` and `presto-passkey` skill variants with install script support for `--wallet=` flag, moved wallet balance to top-level in `whoami` JSON output, improved `InsufficientBalance` error messages with human-readable amounts, and removed automatic browser login prompts in favor of explicit wallet setup instructions.
- Code quality improvements: extracted timeout and polling constants, fixed balance comparison to use numeric equality instead of string matching, added generic `poll_until` helper, added relay status constants, removed unused parameters, and added unit tests for balance change detection and relay deserialization. Removed `--passkey` flag from `wallet create` to keep passkey flow isolated to `presto login`/`presto logout`. Defaulted key authorization to USDC.e only on Tempo mainnet. Refactored install script: renamed `--local` to `--from-source`, removed redundant `--force` and `--passkey` flags, deduplicated agent directory lists and install logic. Removed emoji from CLI output.

### Patch Changes

- Renamed CLI subcommands from singular to plural form: `key` to `keys`, `session` to `sessions`, and `wallet` to `wallets`. Updated all references in source code and tests to use the new plural command names.

## 0.5.0 (2026-02-27)

### Minor Changes

- Enhanced `--sse-json` streaming output to use a structured `{event, data, ts}` NDJSON schema, with automatic JSON parsing of `data:` payloads and ISO-8601 timestamps. Added error event emission on HTTP errors during SSE streaming. Added tests for SSE/NDJSON schema, error events, and curl-parity flags (`--compressed`, `--referer`, `--http2`, `--http1.1`, `--proxy`, `--no-proxy`).
- Added `--connect-timeout`, `--retries`, and `--retry-backoff` CLI flags to support configurable TCP connection timeouts and automatic retry logic with exponential backoff on transient network errors (connect failures and timeouts).
- Added enhanced version info to `--version` output, including git commit hash, build date, and build profile. Added `-j --version` flag support for structured JSON version output with `version`, `git_commit`, `build_date`, and `profile` fields.
- Added `--offline` flag that causes the CLI to fail immediately with a network error without making any HTTP requests. Added corresponding `PrestoError::OfflineMode` variant, exit code mapping, JSON error output support, and tests.
- Added structured JSON error output when `--output-format json` is set, routing errors to stdout as `{ code, message, cause? }` objects with stable machine-readable error code labels. Added `ExitCode::label()` for mapping exit codes to string identifiers, a `render_error_json` helper, and a corresponding integration test. Also suppressed `dead_code` warnings for `OsKeychain` on non-macOS test builds and added a local `coverage` Makefile target.
- Added `--data-urlencode` support with curl-compatible parsing (bare value, `name=value`, `@file`, and `name@file` forms). Extended `-G/--get` to append URL-encoded pairs to the query string alongside `-d` data, and automatically sets `Content-Type: application/x-www-form-urlencoded` when `--data-urlencode` is used without `-G`.
- Refactored key management to use a simplified `KeyEntry` schema, renaming `access_key`/`access_key_address`/`provisioned_chain_ids` to `key`/`key_address`/`provisioned` with new fields for `chain_id`, `key_type`, `expiry`, and `token_limits`. Extracted key authorization logic into a dedicated `key_authorization` module and renamed source files (`wallet.rs` → `local_wallet.rs`, `login.rs` → `passkey_login.rs`) for clarity.

### Patch Changes

- Fixed install script to handle environments where `BASH_SOURCE[0]` is unset by guarding its usage, preventing errors when the script is run via `curl | bash`.
- Fixed sensitive data leaking into verbose logs by redacting URL query parameters and sensitive header values (Authorization, Cookie, X-Api-Key, etc.) before writing to stderr. Added `redact_header_value` and `redact_url` utilities with corresponding unit and integration tests.
- Added `#![forbid(unsafe_code)]` and `#![deny(warnings)]` crate attributes and replaced `unwrap`/`expect` calls in non-test code with proper error handling. Added open source readiness documentation including a phased backlog of quality, security, and agent-UX tasks.
- Updated documentation across README.md, CONTRIBUTING.md, ARCHITECTURE.md, and the agent skill file. Replaced `-q`/`--quiet` and `--output-format json` flags with `-s`/`--silent` and `-j`/`--json-output`, added new CLI options (`--max-pay`, `--currency`, `-L`, `--stream`, `--sse`, `--retries`, etc.), expanded environment variable references, and added a Changelogs section to the contributing guide.
- Added CLI integration tests for config file detection, precedence, and environment variable overrides, covering macOS/Linux paths, explicit `-c` flag, malformed configs, unknown fields, empty configs, RPC overrides, and invalid network flags.
- Fixed browser opening logic to use a plain thread instead of `tokio::task::spawn_blocking` when waiting for user input before opening the URL, preventing the process from hanging after auth completes.
- Replaced on-chain nonce tracking with expiring nonces (TIP-1009) for session transactions. Removed the `get_nonce_for_key` function and NONCE precompile integration, instead using `nonce_key = maxUint256` with a short 25-second validity window to eliminate on-chain nonce queries.
- Updated MPP protocol URL references from `mpp.sh` to `mpp.dev` across documentation and source files.

## 0.4.1 (2026-02-26)

### Patch Changes

- Added multi-network key support so credentials are scoped per network with no cross-network fallback, and introduced client-side pre-broadcast for keychain (smart wallet) channel open transactions with receipt polling. Fixed nonce collisions when closing multiple channels sequentially by tracking per-network nonce offsets, and improved login flow to pass the challenge network through for correct network-aware authentication.
- Fixed multiple security vulnerabilities: prevented payment credential capture via malicious HTTP redirects by using the final URL after redirects for payment retries, added terminal escape sequence sanitization to prevent ANSI injection from server-controlled data, clamped voucher amounts to the known channel deposit to prevent coercion by malicious servers, and added network constraint validation in session requests.

## 0.4.0 (2026-02-25)

### Minor Changes

- Remove `active` field from wallet credentials. Key selection is now deterministic (passkey > first key with access_key > first key) via `primary_key_name()`, with `--key` CLI override. Remove `(active)` marker from `whoami`/`keys` output. Fix provisioning bug where `show_whoami` incorrectly auto-marked keys as provisioned based on spending limit query fallbacks, causing "access key does not exist" errors on re-login. Fix `create_local_wallet` to include token limits for both USDC and pathUSD (mainnet + testnet). Clear `provisioned_chain_ids` on re-login with a new access key. Replace manual date math in `format_expiry_iso` with the `time` crate. Add `presto key` command (`key` = whoami, `key list` = list all keys, `key create` = create fresh access key for local wallets).
- Store wallet EOA private keys in the OS keychain (macOS Keychain) instead of on disk. Access keys from `presto login` remain inline in `keys.toml`. Add `presto wallet` commands for creating, importing, and deleting local wallets.

## 0.3.0 (2026-02-20)

### Minor Changes

- Major simplification by consuming mpp-rs charge builder:
- Replaced manual transaction building and gas resolution with `TempoProvider` from mpp-rs
- Simplified `charge.rs` to single `prepare_charge()` function (~100 lines) using `TempoProvider::pay()`
- Consolidated error mapping (`classify_payment_error`, `map_mpp_validation_error`) into `error.rs`
- Consolidated wallet signer loading into `wallet/signer.rs` as `load_wallet_signer()`
- Inlined escrow open call building in session.rs (no longer depends on mpp-rs swap module)
- Fixed double wallet signer load in session flow (`create_tempo_payment_from_calls` now takes `&WalletSigner`)
- Fixed hyperlink display: transaction hash shown as link text instead of full URL
- Deleted: `payment/currency.rs`, `payment/provider.rs`, `payment/tempo.rs`, `cli/commands/`, `cli/formatting.rs`, `cli/hyperlink.rs`
- Mainnet support: network is now derived from the 402 challenge's `chainId` instead of being hardcoded to testnet. Presto correctly pays on Tempo mainnet (chain 4217) or Moderato testnet (chain 42431) based on what the server requests.
- Additional production hardening:
- `--max-amount` decimal conversion now uses lossless string-based parsing instead of f64 arithmetic, and respects the token's actual decimal places instead of hardcoding 6.
- Session store now uses file locking (`fs2`) to prevent concurrent CLI invocations from clobbering each other's session state.
- Removed duplicate `ChargeRequest` deserialization in the charge flow.
- Analytics network field defaults to `"unknown"` instead of empty string when detection fails.

### Patch Changes

- Replaced brittle string-parsing of MppError messages with typed matching on `MppError::Tempo(TempoClientError)` variants. Deleted `extract_field` and `extract_between` helpers from charge.rs. Payment errors (AccessKeyNotProvisioned, SpendingLimitExceeded, InsufficientBalance, TransactionReverted) now propagate as `PrestoError::Mpp` instead of being stringified into `PrestoError::InvalidChallenge`.
- Replaced local implementations with upstream mpp-rs helpers:
- Challenge expiry checking now uses `PaymentChallenge::is_expired()` instead of a hand-rolled RFC 3339 parser
- Spending limit queries (`query_key_spending_limit`, `local_key_spending_limit`) now use `mpp::client::tempo::keychain` instead of local copies
- Removed `IAccountKeychain` ABI and `KEYCHAIN_ADDRESS` from local `abi.rs` in favor of upstream
- ABI encoding helpers (`encode_transfer`, `encode_approve`, `encode_swap_exact_amount_out`, `DEX_ADDRESS`) now re-exported from `mpp::protocol::methods::tempo::abi`
- Challenge validation (`validate_challenge`, `validate_session_challenge`) now delegates to upstream `PaymentChallenge::validate_for_charge/session()`
- `extract_tx_hash` now delegates to `mpp::protocol::core::extract_tx_hash`
- `parse_memo` now delegates to `mpp::protocol::methods::tempo::charge::parse_memo_bytes`
- `SwapInfo` and slippage constants now re-exported from `mpp::client::tempo::swap`
- Removed local `is_supported_method` helper (upstream validation handles method checking)

## 0.2.0 (2026-02-18)

### Minor Changes

- ### Features
- Support `@filename` and `@-` (stdin) syntax for `-d`/`--data` flag
- Improved CLI ergonomics (shorter flags, better defaults)
- Gas estimation for Tempo transactions with account creation cost handling
- Key spending limit checks before payment and swap
- Local access key generation (no longer received from browser)
- Analytics with PostHog for funnel dashboards
- Examples directory for common usage patterns
- Eval framework for testing
- ### Fixes
- SSE streaming no longer hangs
- Atomic file writes for config and wallet files
- Gas estimation uses fixed buffer instead of percentage
- Gas limit bumped to 500k for Account Abstraction transactions
- Show AI integrations message only on new install
- ### Refactors
- Renamed to presto
- Streamlined CLI — removed config/version commands, merged keys into whoami
- Removed keystore/private key support, passkey auth only
- Cleaned up payment error messages

### Patch Changes

- Increased default `max_fee_per_gas` from 10 gwei to 20 gwei for improved transaction reliability.
- Fix spending limit queries to fail-closed instead of fail-open. Previously, RPC failures in `query_key_spending_limit` returned `None` (treated as unlimited), allowing payments with unauthorized tokens. Now returns `Result<Option<U256>>` — RPC errors block payment, `Ok(None)` means genuinely unlimited, `Ok(Some(n))` means enforced limit. Swap source selection also rejects tokens with failed limit queries.
- Increased default gas limit from 100,000 to 300,000 to support Account Abstraction transactions.

