---
tempo-wallet: patch
---

Slim the service list schema to replace full endpoint details with an `endpoint_count` field, reducing payload size. Adds a test to enforce the summary-only structure.
