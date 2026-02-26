# OSR-04: Standardize Machine-Readable Outputs

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Ensure every command supports `--json` with a stable schema. JSON goes to stdout; logs/diagnostics to stderr. Add `--quiet`, `--no-color`, `--log-format json|text`.

Scope
- `src/cli/output.rs`, all CLI commands under `src/cli/**`.

Deliverables
- Stable JSON schemas per command documented and tested.
- Stderr/stdout separation enforced; logging flags wired.

Acceptance Criteria
- Integration tests assert JSON shape and stderr separation.
- Help text documents all relevant flags.

Test Plan
- Add black-box CLI tests for representative commands validating stdout JSON and stderr logs.

Risks/Notes
- Be careful to avoid breaking existing text output; keep `--text` as default if present.
