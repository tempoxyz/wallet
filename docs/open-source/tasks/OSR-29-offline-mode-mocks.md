# OSR-29: Offline Mode and Deterministic Mocks

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Add `--offline` flag to force mocks and prevent network. Ensure tests and CI paths rely on offline mode for determinism.

Scope
- CLI flags, HTTP client runtime, test harness.

Deliverables
- Offline mode behavior with clear errors on attempted network access.

Acceptance Criteria
- CI uses offline paths reliably; tests deterministic across runs.

Test Plan
- Integration tests that fail if network attempted; verify mock usage.

Risks/Notes
- Keep behavior explicit to avoid surprising users.
