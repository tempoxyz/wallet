# OSR-12: HTTP Client and 402→Payment→Response Flow

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Black-box tests for the 200 path, 402 one-shot payment, 402 session payment, and error cases using a mock server.

Scope
- `src/http.rs`, `src/cli/query.rs`, payment integration surfaces.

Deliverables
- Integration tests asserting request/response headers, redactions, and retry semantics.

Acceptance Criteria
- CLI tests validate end-to-end behavior across all branches.

Test Plan
- Use mock server crates; simulate 402 with charge/session flows.

Risks/Notes
- Ensure no network in CI; all flows mocked.
