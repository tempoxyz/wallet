# Changelog

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

