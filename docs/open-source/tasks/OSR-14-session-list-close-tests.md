# OSR-14: Session List/Close Commands

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Black-box tests for `session list` JSON rendering and `session close` idempotency.

Scope
- `src/cli/session/**`.

Deliverables
- Tests verifying stable JSON and repeated close behavior.

Acceptance Criteria
- Re-running `close` yields stable exit code and message; list output is schema-stable.

Test Plan
- CLI tests with mock session store and operations.

Risks/Notes
- Ensure no network; store interactions are mocked.
