# OSR-16: Output Rendering Tests

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Golden tests for `--json` and `--text` outputs with snapshots to ensure stability for agents.

Scope
- `src/cli/output.rs`, CLI surfaces producing output.

Deliverables
- Snapshot fixtures and tests; documented schema for JSON outputs.

Acceptance Criteria
- Stable snapshots; changes require explicit approval via snapshot update.

Test Plan
- Use insta or similar snapshot testing; normalize nondeterministic fields.

Risks/Notes
- Avoid embedding volatile timestamps unless normalized.
