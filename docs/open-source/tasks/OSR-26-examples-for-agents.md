# OSR-26: Examples for Agents

Status: Planned
Phase: 5
Owner: Unassigned

Summary
- Provide minimal Bash and Python examples for streaming LD-JSON, retry loops, and stdin payloads (`--json @-`). CI runs them in a mocked environment.

Scope
- `examples/` directory and CI smoke tests.

Deliverables
- Working examples and CI that exercises them without real network.

Acceptance Criteria
- CI smoke tests pass; outputs are deterministic.

Test Plan
- Use mock servers and fixtures; assert expected stdout.

Risks/Notes
- Keep examples small; avoid duplication with SKILL.md.
