---
presto: minor
---

Refactored key management to use a simplified `KeyEntry` schema, renaming `access_key`/`access_key_address`/`provisioned_chain_ids` to `key`/`key_address`/`provisioned` with new fields for `chain_id`, `key_type`, `expiry`, and `token_limits`. Extracted key authorization logic into a dedicated `key_authorization` module and renamed source files (`wallet.rs` → `local_wallet.rs`, `login.rs` → `passkey_login.rs`) for clarity.
