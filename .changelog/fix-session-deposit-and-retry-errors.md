---
tempo-request: patch
---

Fix session deposit and key-auth retry error handling.

- Preserve original error when key-authorization retry also fails, instead of showing misleading retry errors (e.g. `KeyAlreadyExists`).
- Ensure session deposit covers at least the per-request cost, preventing channels from opening with insufficient funds.
- Fail early before opening a channel if wallet balance is too low to cover the request cost.
