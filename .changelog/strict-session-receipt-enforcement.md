---
tempo-request: patch
---

Enforce strict session `Payment-Receipt` handling for v2/new sessions across streaming paths: reject invalid SSE `payment-receipt` events and reject successful streaming top-up responses that omit/invalidly encode receipts. Keep legacy session behavior permissive while bounding persisted `spent` values, and extend integration coverage for strict streaming top-up missing-receipt failures.
