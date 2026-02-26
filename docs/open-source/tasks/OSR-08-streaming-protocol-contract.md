# OSR-08: Streaming Protocol Contract

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Define and implement line-delimited JSON streaming with `{ event, data, ts }` across streaming features.

Scope
- Streaming modules under `src/payment/session/streaming.rs` and any CLI surfaces.

Deliverables
- Emit LD-JSON events with stable schema; document contract.

Acceptance Criteria
- Integration tests consume streams and assert schema and ordering.

Test Plan
- Black-box tests that parse lines and validate structure and timestamps.

Risks/Notes
- Ensure backward-compatible evolution via `event` names and optional fields.
