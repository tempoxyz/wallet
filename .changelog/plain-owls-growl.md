---
presto: minor
---

Added structured JSON error output when `--output-format json` is set, routing errors to stdout as `{ code, message, cause? }` objects with stable machine-readable error code labels. Added `ExitCode::label()` for mapping exit codes to string identifiers, a `render_error_json` helper, and a corresponding integration test. Also suppressed `dead_code` warnings for `OsKeychain` on non-macOS test builds and added a local `coverage` Makefile target.
