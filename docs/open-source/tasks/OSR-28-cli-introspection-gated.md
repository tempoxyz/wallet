# OSR-28: CLI Introspection Gated by Env

Status: Planned
Phase: 6
Owner: Unassigned

Summary
- Add `PRESTO_AGENT_INTROSPECT=1 presto --introspect` to produce a JSON schema of commands/flags without exposing hidden commands in normal help.

Scope
- CLI command plumbing and feature gating by env var.

Deliverables
- Introspection output in stable JSON; not included in regular help.

Acceptance Criteria
- Only available when env var set; JSON validates against schema.

Test Plan
- Unit/integration tests with/without env var.

Risks/Notes
- Do not document exact invocation in public docs; mention behavior at high level only.
