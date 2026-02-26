---
presto: patch
---

Added multi-network key support so credentials are scoped per network with no cross-network fallback, and introduced client-side pre-broadcast for keychain (smart wallet) channel open transactions with receipt polling. Fixed nonce collisions when closing multiple channels sequentially by tracking per-network nonce offsets, and improved login flow to pass the challenge network through for correct network-aware authentication.
