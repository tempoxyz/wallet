# OSR-01: Enforce Lints and Safety Gates

Status: Planned
Phase: 1
Owner: Unassigned

Summary
- Enforce crate-wide lints to prevent unsafe code and warnings; remove `unwrap`/`expect` from non-test code.

Scope
- `src/main.rs`, public modules under `src/**.rs`.
- Excludes `tests/**` and benches.

Deliverables
- Add crate attributes where applicable: `#![forbid(unsafe_code)]`, `#![deny(warnings)]`.
- Replace `unwrap`/`expect` in non-test code with typed errors using `thiserror`/`anyhow`.

Acceptance Criteria
- `make check` succeeds with zero warnings.
- `rg -n "[.]unwrap\(|[.]expect\(" src | wc -l` returns 0 (excluding tests/benches).

Test Plan
- Run `make check` locally and in CI.
- Update/add tests only where error flow changes.

Risks/Notes
- May require minor refactors of error enums and mapping.
