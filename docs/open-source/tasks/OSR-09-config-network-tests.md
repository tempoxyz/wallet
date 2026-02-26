# OSR-09: Config Parsing and Network Resolution Tests

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Test precedence of network resolution (env > typed overrides > general table > default). Add property-based tests for TOML maps.

Scope
- `src/config.rs`, `src/network.rs`.

Deliverables
- Unit tests for precedence rules and edge cases.
- Proptests for arbitrary TOML inputs to ensure determinism and no panics.

Acceptance Criteria
- Coverage > 90% for config/network modules.

Test Plan
- Fast, deterministic tests; avoid external IO.

Risks/Notes
- Guard flaky time-dependent behaviors; no network.
