# OSR-07: Retries, Timeouts, Backoff Flags

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Add global flags for `--connect-timeout`, `--timeout`, `--retry N`, `--retry-backoff ms` with sensible defaults for agents.

Scope
- `src/http.rs`, `src/cli/query.rs`, shared config for HTTP runtime.

Deliverables
- Implement flags and backoff strategy; document defaults.

Acceptance Criteria
- Mocked HTTP tests validate timeout handling and retry/backoff behavior.

Test Plan
- Integration tests using a mock server with delayed/error responses.

Risks/Notes
- Avoid retry storms; cap retries/backoff.
