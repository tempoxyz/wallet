# OSR-21: SAST with Semgrep and CodeQL

Status: Planned
Phase: 4
Owner: Unassigned

Summary
- Add Semgrep ruleset (including custom rules for redaction and panic prohibition) and CodeQL workflow for Rust. Start as informational-only in Phase 2 (non-blocking), then enforce in Phase 4 after baseline triage.

Scope
- Repo root CI workflows, Semgrep config under `.semgrep/`.

Deliverables
- Semgrep CI with custom rules; CodeQL workflow enabled.
- Phase 2: CI posts findings without failing PRs; triage baseline.
- Phase 4: Elevate to blocking on high-severity findings.

Acceptance Criteria
- CI passes with no high-severity findings; new findings require triage.

Test Plan
- Seed safe examples to validate rule matches and non-matches.

Risks/Notes
- Keep false positives low; document suppressions.
