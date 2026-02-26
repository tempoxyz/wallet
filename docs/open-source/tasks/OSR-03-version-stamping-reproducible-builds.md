# OSR-03: Version Stamping and Reproducible Builds

Status: Planned
Phase: 1
Owner: Unassigned

Summary
- Embed git commit and build metadata in `presto --version`. Ensure release builds are stripped and as deterministic as feasible.

Scope
- `src/main.rs` or build-time module; `Cargo.toml`/build scripts if needed.

Deliverables
- `presto --version` prints `presto x.y.z (commit <SHA>, <date>)`.
- Configure release profile: strip, deterministic linker flags when supported.

Acceptance Criteria
- Manual run of `presto --version` shows expected stamp.
- CI release builds have deterministic metadata fields where supported.

Test Plan
- Add a unit test that parses `--version` output format (string pattern).

Risks/Notes
- Full reproducibility across platforms may not be 100%; document scope.
