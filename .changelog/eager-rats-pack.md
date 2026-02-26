---
presto: minor
---

Added `--connect-timeout`, `--retries`, and `--retry-backoff` CLI flags to support configurable TCP connection timeouts and automatic retry logic with exponential backoff on transient network errors (connect failures and timeouts).
