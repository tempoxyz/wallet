---
tempo-wallet: patch
---

Removed the "all" amount option from the transfer command, along with the associated `query_balance` helper and `balanceOf` interface method. The `resolve_amount` function is now synchronous.
