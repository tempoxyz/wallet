---
tempo-common: patch
tempo-request: patch
tempo-wallet: patch
---

Bump to the latest `mpp-rs` `main`, including upstream security fixes for payment bypass, replay, fee-payer manipulation, and session/channel griefing vulnerabilities across Tempo and Stripe flows. The update also refreshes the Tempo client dependency graph and switches Tempo gas estimation to a request-based API while preserving existing wallet/request behavior.
