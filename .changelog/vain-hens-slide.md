---
presto: patch
---

Optimized request path to reuse a single HTTP client across initial and payment replay requests, enabling TCP/TLS connection pooling and skipping redundant TLS handshakes. Replaced on-chain nonce fetches with expiring nonces (`nonceKey=MAX`), removed the `find_channel_on_chain` recovery path, simplified the keychain/direct-signing flow to a unified path, and scoped verbose logging to the `presto` crate only.
