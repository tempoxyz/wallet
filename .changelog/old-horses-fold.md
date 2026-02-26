---
presto: minor
---

Added `--offline` flag that causes the CLI to fail immediately with a network error without making any HTTP requests. Added corresponding `PrestoError::OfflineMode` variant, exit code mapping, JSON error output support, and tests.
