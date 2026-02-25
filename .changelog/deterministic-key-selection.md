---
"presto": minor
---

Remove `active` field from wallet credentials. Key selection is now deterministic (passkey > first key with access_key > first key) via `primary_key_name()`, with `--key` CLI override. Remove `(active)` marker from `whoami`/`keys` output. Fix provisioning bug where `show_whoami` incorrectly auto-marked keys as provisioned based on spending limit query fallbacks, causing "access key does not exist" errors on re-login. Fix `create_local_wallet` to include token limits for both USDC and pathUSD (mainnet + testnet). Clear `provisioned_chain_ids` on re-login with a new access key. Replace manual date math in `format_expiry_iso` with the `time` crate. Add `presto key` command (`key` = whoami, `key list` = list all keys, `key create` = create fresh access key for local wallets).
