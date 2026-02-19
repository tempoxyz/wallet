---
"presto": patch
---

Replaced local implementations with upstream mpp-rs helpers:
- Challenge expiry checking now uses `PaymentChallenge::is_expired()` instead of a hand-rolled RFC 3339 parser
- Spending limit queries (`query_key_spending_limit`, `local_key_spending_limit`) now use `mpp::client::tempo::keychain` instead of local copies
- Removed `IAccountKeychain` ABI and `KEYCHAIN_ADDRESS` from local `abi.rs` in favor of upstream
- ABI encoding helpers (`encode_transfer`, `encode_approve`, `encode_swap_exact_amount_out`, `DEX_ADDRESS`) now re-exported from `mpp::protocol::methods::tempo::abi`
- Challenge validation (`validate_challenge`, `validate_session_challenge`) now delegates to upstream `PaymentChallenge::validate_for_charge/session()`
- `extract_tx_hash` now delegates to `mpp::protocol::core::extract_tx_hash`
- `parse_memo` now delegates to `mpp::protocol::methods::tempo::charge::parse_memo_bytes`
- `SwapInfo` and slippage constants now re-exported from `mpp::client::tempo::swap`
- Removed local `is_supported_method` helper (upstream validation handles method checking)
