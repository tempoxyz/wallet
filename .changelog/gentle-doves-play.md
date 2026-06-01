---
tempo-common: minor
tempo-request: minor
tempo-wallet: minor
---

Adds EIP-1186 MPT proof verification for on-chain reads. Introduces a new `proof` module in `tempo-common` with `verified_storage_at`, `verified_account_balance`, and `verified_token_balance` helpers, and adds a `balance_mapping_slot` field to `TokenConfig`. `tempo-request` and `tempo-wallet` now use verified reads via `eth_getProof`, falling back to unverified `eth_call` if proof pinning fails.
