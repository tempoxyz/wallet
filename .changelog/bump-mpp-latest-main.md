---
tempo-common: patch
tempo-request: patch
tempo-wallet: patch
---

Bump to the latest `mpp-rs` `main`, which updates the Tempo client dependency graph and switches Tempo gas estimation to a request-based API. This keeps wallet and request flows building against the newer MPP client while preserving existing HTTP/2 and session transaction behavior.
