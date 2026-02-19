---
"presto": minor
---

Major simplification by consuming mpp-rs charge builder and auto-swap routing:

- Replaced manual transaction building, gas resolution, swap routing, and balance checking with `TempoProvider` from mpp-rs
- Simplified `charge.rs` to single `prepare_charge()` function (~100 lines) using `TempoProvider::pay()`
- Consolidated error mapping (`classify_payment_error`, `map_mpp_validation_error`) into `error.rs`
- Consolidated wallet signer loading into `wallet/signer.rs` as `load_wallet_signer()`
- Fixed double wallet signer load in session flow (`create_tempo_payment_from_calls` now takes `&WalletSigner`)
- Fixed hyperlink display: transaction hash shown as link text instead of full URL
- Deleted: `payment/currency.rs`, `payment/provider.rs`, `payment/tempo.rs`, `cli/commands/`, `cli/formatting.rs`, `cli/hyperlink.rs`
