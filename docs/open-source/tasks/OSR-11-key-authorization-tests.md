# OSR-11: Key Authorization Decode/Validate/Sign Tests

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Cover valid/invalid RLP payloads, signature mismatch, expiry boundaries for key authorization.

Scope
- `src/wallet/key_authorization.rs`.

Deliverables
- Unit tests for decode/validate/sign, including negative cases and boundary times.

Acceptance Criteria
- All branches covered; inputs do not panic on malformed data.

Test Plan
- Construct fixtures for RLP and signatures; use deterministic keys.

Risks/Notes
- Ensure no secret material leaks in test logs.
