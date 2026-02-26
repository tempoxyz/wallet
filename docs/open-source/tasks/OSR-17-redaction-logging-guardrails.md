# OSR-17: Redaction and Logging Guardrails

Status: Planned
Phase: 4
Owner: Unassigned

Summary
- Centralize redaction for sensitive data and prohibit direct logging of secrets across the codebase.

Scope
- Logging across modules; shared redact helpers.

Deliverables
- Redaction helpers for `Authorization`, `X-API-Key`, bearer tokens, secrets.
- Replace any direct sensitive logging with redacted variants.
- Semgrep rule to detect sensitive logging.

Acceptance Criteria
- Unit tests cover redaction helpers; Semgrep CI gate passes.

Test Plan
- Redaction unit tests for representative inputs; Semgrep runs in CI.

Risks/Notes
- Keep performance overhead minimal; avoid double redaction.
