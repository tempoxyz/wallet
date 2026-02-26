# OSR-30: Telemetry Schema and Sampling

Status: Planned
Phase: 6
Owner: Unassigned

Summary
- Document telemetry event schema; confirm no payload/body data is ever sent. Optionally add sampling controls.

Scope
- `src/analytics/**`, docs.

Deliverables
- Public schema and tests enforcing field set; optional sampling toggle.

Acceptance Criteria
- Telemetry tests assert schema adherence; no sensitive data transmitted.

Test Plan
- Unit tests on event building and filters; snapshot schema.

Risks/Notes
- Keep telemetry minimal and privacy-preserving.
