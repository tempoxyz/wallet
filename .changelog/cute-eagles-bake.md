---
presto: patch
---

Added `#![forbid(unsafe_code)]` and `#![deny(warnings)]` crate attributes and replaced `unwrap`/`expect` calls in non-test code with proper error handling. Added open source readiness documentation including a phased backlog of quality, security, and agent-UX tasks.
