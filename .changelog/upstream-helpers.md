---
"presto": patch
---

Replaced local implementations with upstream mpp-rs helpers:
- Challenge expiry checking now uses `PaymentChallenge::is_expired()` instead of a hand-rolled RFC 3339 parser
- Spending limit queries (`query_key_spending_limit`, `local_key_spending_limit`) now use `mpp::client::tempo::keychain` instead of local copies
- Removed `IAccountKeychain` ABI and `KEYCHAIN_ADDRESS` from local `abi.rs` in favor of upstream
