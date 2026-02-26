# OSR-02: CI Coverage, Audit, Deny, MSRV

Status: Planned
Phase: 1
Owner: Unassigned

Summary
- Add CI jobs for coverage (cargo llvm-cov), vulnerability audit (cargo audit), license/compliance (cargo deny), and MSRV matrix.

Scope
- CI config in `.github/workflows/**`, `Makefile` targets.

Deliverables
- Coverage job that outputs HTML/LCOV artifacts and enforces ≥85% threshold. Add `make coverage`.
- `cargo audit` and `cargo deny` jobs; fail PRs on findings.
- CI matrix: stable and MSRV (documented), optional beta.

Acceptance Criteria
- Coverage artifacts available from CI; job fails below threshold.
- `cargo audit` and `cargo deny` pass on main.

Test Plan
- Dry-run CI locally where applicable; open PR to validate workflows.

Risks/Notes
- Threshold may need iteration to avoid flakiness; pin llvm-tools version.
