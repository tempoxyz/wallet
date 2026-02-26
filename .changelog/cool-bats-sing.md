---
presto: patch
---

Fixed sensitive data leaking into verbose logs by redacting URL query parameters and sensitive header values (Authorization, Cookie, X-Api-Key, etc.) before writing to stderr. Added `redact_header_value` and `redact_url` utilities with corresponding unit and integration tests.
