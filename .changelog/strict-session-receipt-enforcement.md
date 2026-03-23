---
tempo-request: patch
---

Enforce strict session `Payment-Receipt` handling across all session flows, including reused persisted sessions: reject successful paid responses that omit/malformedly encode receipts, require valid `spent` semantics for response/header/event receipts, and preserve conservative local channel state when strict top-up receipt validation fails after a paid response. Extend integration coverage for strict open/top-up/streaming receipt failure paths.
