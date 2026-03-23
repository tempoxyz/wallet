---
tempo-request: patch
---

Enforce strict session `Payment-Receipt` handling for v2/new sessions across streaming paths: reject invalid SSE `payment-receipt` events and reject successful streaming top-up responses that omit/invalidly encode receipts. Keep legacy session behavior permissive while bounding persisted `spent` values, persist a conservative local deposit floor when strict top-up receipt validation fails after a paid response, and extend integration coverage for strict streaming top-up missing-receipt failures.
