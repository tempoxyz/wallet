---
presto: patch
---

Refactored internal module structure by splitting `src/util.rs` into focused submodules (`util/format.rs`, `util/fs.rs`, `util/terminal.rs`), extracting `dispatch_payment` and `parse_payment_rejection` into `src/payment/dispatch.rs`, and moving CLI helpers (`init_tracing`, `generate_completions`, `print_version_json`) into `src/cli/logging.rs` and `src/cli/completions.rs`. Also moved retry logic from `query.rs` into `HttpRequestPlan` via a new `RetryPolicy` struct.
