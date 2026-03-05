---
tempo-wallet: minor
---

Refactored session management to consolidate close state into session records, replacing the separate `pending_closes` table with explicit `state`, `close_requested_at`, `grace_ready_at`, and `token_decimals` fields on `SessionRecord`. Added per-origin file locking (`fs2`) to prevent duplicate channel opens across processes, improved cooperative close to fetch a fresh challenge echo before submitting, and introduced "closing"/"finalizable" session statuses. Also fixed deposit scaling to respect token decimals and corrected the `whoami` ready flag to require a wallet.
