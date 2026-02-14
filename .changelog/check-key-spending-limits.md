---
presto-cli: patch
---

Fix spending limit queries to fail-closed instead of fail-open. Previously, RPC failures in `query_key_spending_limit` returned `None` (treated as unlimited), allowing payments with unauthorized tokens. Now returns `Result<Option<U256>>` — RPC errors block payment, `Ok(None)` means genuinely unlimited, `Ok(Some(n))` means enforced limit. Swap source selection also rejects tokens with failed limit queries.
