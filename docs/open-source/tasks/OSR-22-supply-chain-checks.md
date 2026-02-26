# OSR-22: Supply Chain Checks

Status: Planned
Phase: 4
Owner: Unassigned

Summary
- Focus on `cargo-vet` attestations and transitive dependency patching. `cargo audit` and `cargo deny` are added in OSR-02 and enforced there.

Scope
- CI workflows, `Cargo.toml` (for `[patch.crates-io]` as needed).

Deliverables
- `cargo-vet` configured with initial policy; document exceptions.
- Apply `[patch.crates-io]` where transitive pinning is required.

Acceptance Criteria
- Vet policy exists; CI cross-check runs.
- Where patches are needed, they are documented and tested.

Test Plan
- Dry-run `cargo vet diff` on updates; verify CI action runs.

Risks/Notes
- Keep deny lists curated; document exceptions and rationale.
