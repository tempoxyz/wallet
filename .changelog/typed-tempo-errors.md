---
"presto": patch
---

Replaced brittle string-parsing of MppError messages with typed matching on `MppError::Tempo(TempoClientError)` variants. Deleted `extract_field` and `extract_between` helpers from charge.rs. Payment errors (AccessKeyNotProvisioned, SpendingLimitExceeded, InsufficientBalance, TransactionReverted) now propagate as `PrestoError::Mpp` instead of being stringified into `PrestoError::InvalidChallenge`.
