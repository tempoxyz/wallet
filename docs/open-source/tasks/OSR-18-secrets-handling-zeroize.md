# OSR-18: Secrets Handling Improvements

Status: Planned
Phase: 4
Owner: Unassigned

Summary
- Evaluate `zeroize` for in-memory key material; ensure keychain flows do not log secrets and use typed errors.

Scope
- `src/wallet/keychain.rs`, `src/wallet/credentials/**`.

Deliverables
- Adopt `zeroize` where feasible; audit logs; improve error typing.

Acceptance Criteria
- Keys zeroized on drop where applicable; tests confirm no accidental prints.

Test Plan
- Unit tests for zeroization behavior and failure paths without leaks.

Risks/Notes
- Limit scope to hot paths to avoid large refactors.
