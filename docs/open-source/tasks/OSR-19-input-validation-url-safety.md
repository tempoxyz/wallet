# OSR-19: Input Validation and URL Safety

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Validate URL schemes (`http`/`https`), disallow `file:`/`data:` by default. Block localhost unless `--allow-localhost`. Prevent header injection via `\r\n`.

Scope
- `src/http.rs`, `src/cli/query.rs`.

Deliverables
- URL and headers validation with clear error messages.

Acceptance Criteria
- Unit tests for invalid URLs and header names/values; stable exit codes and messages.

Test Plan
- Negative tests per invalid case; ensure no network calls made.

Risks/Notes
- Provide explicit escape hatches for power users behind flags.
