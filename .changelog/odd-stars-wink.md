---
tempo-wallet: patch
---

Refactored payment dispatch to centralize challenge parsing and network/signer resolution in `dispatch_payment`, passing a `ResolvedChallenge` struct to `handle_charge_request` and `handle_session_request`. Moved `parse_payment_rejection` to `charge.rs`, removed the `SessionResult` enum in favor of `PaymentResult`, and eliminated redundant `Config` and network lookups from individual payment handlers.
