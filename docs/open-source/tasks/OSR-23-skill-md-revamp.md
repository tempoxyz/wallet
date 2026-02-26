# OSR-23: SKILL.md Revamp for AI Agents

Status: Planned
Phase: 5
Owner: Unassigned

Summary
- Rewrite `.agents/skills/presto/SKILL.md` with agent-focused recipes: auto-402 handling, "ask the oracle" OpenRouter example, session payments, streaming LD-JSON, error handling via exit codes, retries/timeouts, and redaction guidance. Explicitly exclude hidden/experimental commands.

Scope
- `.agents/skills/presto/SKILL.md`, examples referenced within.

Deliverables
- Clear, copy-pasteable agent recipes with stable flags and JSON contracts.
 - Approach: Draft early (post-Phase 2) to align on interface; finalize after Phase 2 contracts are locked to avoid churn.

Acceptance Criteria
- Internal agent harness validates recipes end-to-end.

Test Plan
- Run documented commands in a mock environment; snapshot outputs.

Risks/Notes
- Keep examples minimal and deterministic; avoid leaking secrets.
