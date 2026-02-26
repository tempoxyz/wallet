---
presto: minor
---

Added `--data-urlencode` support with curl-compatible parsing (bare value, `name=value`, `@file`, and `name@file` forms). Extended `-G/--get` to append URL-encoded pairs to the query string alongside `-d` data, and automatically sets `Content-Type: application/x-www-form-urlencoded` when `--data-urlencode` is used without `-G`.
