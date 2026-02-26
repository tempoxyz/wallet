# OSR-20: Fuzzing Targets with cargo-fuzz

Status: Planned
Phase: 4
Owner: Unassigned

Summary
- Add fuzz targets for critical parsers/transformers: config TOML parse, credentials TOML parse, RLP decode, channel transitions, HTTP header construction.

Scope
- `fuzz/` directory with targets; minimal harness code in repo.

Deliverables
- Working fuzz targets; CI nightly smoke run with small time budget.

Acceptance Criteria
- Fuzzers compile and run locally; CI executes smoke runs successfully.

Test Plan
- Local runs with short timeouts; CI cron workflow.

Risks/Notes
- Avoid network and nondeterminism in harnesses.
