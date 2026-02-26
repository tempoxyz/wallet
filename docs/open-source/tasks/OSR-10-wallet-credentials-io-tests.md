# OSR-10: Wallet Credentials Model and IO Tests

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Test key selection priority (passkey > first with `key` > first lexicographic). Enforce file perms `0600` and reject weaker perms (Unix-guarded). Property-test token limit parsing.

Scope
- `src/wallet/credentials/model.rs`, `src/wallet/credentials/io.rs`.

Deliverables
- Deterministic selection tests; IO rejects insecure perms; property tests for token limits.

Acceptance Criteria
- All branches covered; no panics.

Test Plan
- Unit tests with temporary files; `#[cfg(unix)]` for perms tests.

Risks/Notes
- Platform differences (macOS/Linux) for permissions; guard appropriately.
