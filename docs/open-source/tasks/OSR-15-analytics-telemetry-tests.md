# OSR-15: Analytics/Telemetry Tests

Status: Planned
Phase: 3
Owner: Unassigned

Summary
- Verify `PRESTO_NO_TELEMETRY` disables events; add redaction tests for headers, URLs, and payloads.

Scope
- `src/analytics/**`.

Deliverables
- Unit tests covering event building, redaction, and env-based disablement.

Acceptance Criteria
- No network activity during tests; all sensitive fields redacted.

Test Plan
- Mock transport; assert payload content and filters.

Risks/Notes
- Keep logs free of PII; use central redaction helpers.
