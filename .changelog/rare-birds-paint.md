---
presto: patch
---

Replaced on-chain nonce tracking with expiring nonces (TIP-1009) for session transactions. Removed the `get_nonce_for_key` function and NONCE precompile integration, instead using `nonce_key = maxUint256` with a short 25-second validity window to eliminate on-chain nonce queries.
