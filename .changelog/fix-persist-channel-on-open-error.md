---
tempo-request: patch
---

Persist channel state when channel open fails to prevent orphaned on-chain funds.

When the open transaction is sent to the server but the server returns an error, the channel may already exist on-chain. Previously the channel state was lost, making the deposited funds unrecoverable. Now the channel record is persisted before returning the error, so future runs can discover and close it.
