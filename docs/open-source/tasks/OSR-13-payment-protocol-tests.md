# OSR-13: Payment Protocol Tests

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Unit tests for fee math, channel state transitions, close semantics; integration tests with mocked MPP SDK responses including streaming.

Scope
- `src/payment/charge.rs`, `src/payment/session/**`.

Deliverables
- High-coverage unit tests and mocked integration flows.

Acceptance Criteria
- Deterministic channel close; error propagation verified in tests.

Test Plan
- Mock MPP SDK interfaces; assert state machine invariants.

Risks/Notes
- Be careful with time/nonce assumptions in tests; stabilize with fixtures.
