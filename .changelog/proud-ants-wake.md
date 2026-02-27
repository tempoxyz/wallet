---
presto: patch
---

Optimized network and RPC calls to run concurrently throughout the CLI. Replaced sequential async calls with `tokio::join!` and `futures::future::join_all` for balance queries, spending limit lookups, channel state fetches, and multi-network channel scans. Also skipped the expensive on-chain channel scan for genuinely new sessions and replaced a load-then-save session pattern with an atomic upsert helper.
