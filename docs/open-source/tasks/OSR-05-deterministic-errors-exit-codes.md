# OSR-05: Deterministic Errors and Exit Codes

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Map all error paths to structured JSON (`{ code, message, hint, cause? }`) and to stable exit codes per class.

Scope
- `src/cli/exit_codes.rs`, `src/error.rs`, command handlers that emit errors.

Deliverables
- Error-to-exit-code mapping table and implementation.
- JSON error bodies returned when `--json` is set; human-friendly text otherwise.

Acceptance Criteria
- Integration tests intentionally trigger each error class and assert JSON body and exit code.

Test Plan
- Black-box CLI tests per representative error category.

Risks/Notes
- Avoid leaking secrets in error messages; use redaction helpers (see OSR-17).
