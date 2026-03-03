---
presto: patch
---

Added `sessions sync` command to reconcile local session records with on-chain state. Added locked/available balance breakdown to `whoami` and `keys` output showing funds held in active payment channels. Improved session close output messages with cleaner formatting and settlement transaction URLs. Removed session TTL expiry logic and migrated the sessions database schema to drop the `expires_at` column. Added `created_at` and `last_used_at` timestamps to session list output.
