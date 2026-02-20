# Changelog

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
- Mainnet support: network is now derived from the 402 challenge's `chainId` instead of being hardcoded to testnet.  Tempo Walletcorrectly pays on Tempo mainnet (chain 4217) or Moderato testnet (chain 42431) based on what the server requests.
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

