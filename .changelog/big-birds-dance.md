---
presto: minor
---

Enhanced `--sse-json` streaming output to use a structured `{event, data, ts}` NDJSON schema, with automatic JSON parsing of `data:` payloads and ISO-8601 timestamps. Added error event emission on HTTP errors during SSE streaming. Added tests for SSE/NDJSON schema, error events, and curl-parity flags (`--compressed`, `--referer`, `--http2`, `--http1.1`, `--proxy`, `--no-proxy`).
