---
tempo-common: patch
tempo-request: patch
---

Only purge local credentials/session state for inactive access-key errors after confirming key state on-chain. This prevents destructive cleanup on transient or misclassified failures and improves error classification for charge/session payment paths.
