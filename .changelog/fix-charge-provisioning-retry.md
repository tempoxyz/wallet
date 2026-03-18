---
tempo-wallet: patch
tempo-request: patch
---

Fix charge payment failing with "access key does not exist" when the signing key is not yet provisioned on-chain. The server-side rejection retry only triggered on HTTP 401-403, but the server returns other status codes for keychain errors.
