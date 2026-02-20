---
"presto": minor
---

Major simplification by consuming mpp-rs charge builder:

- Replaced manual transaction building and gas resolution with `TempoProvider` from mpp-rs
- Simplified `charge.rs` to single `prepare_charge()` function (~100 lines) using `TempoProvider::pay()`
- Consolidated error mapping (`classify_payment_error`, `map_mpp_validation_error`) into `error.rs`
- Consolidated wallet signer loading into `wallet/signer.rs` as `load_wallet_signer()`
- Inlined escrow open call building in session.rs (no longer depends on mpp-rs swap module)
- Fixed double wallet signer load in session flow (`create_tempo_payment_from_calls` now takes `&WalletSigner`)
- Fixed hyperlink display: transaction hash shown as link text instead of full URL
- Deleted: `payment/currency.rs`, `payment/provider.rs`, `payment/tempo.rs`, `cli/commands/`, `cli/formatting.rs`, `cli/hyperlink.rs`
