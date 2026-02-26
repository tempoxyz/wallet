# OSR-06: Help UX Without Leaking Hidden Commands

Status: Planned
Phase: 2
Owner: Unassigned

Summary
- Ensure internal/experimental commands are hidden in `--help`. Optionally provide `--help-md` or machine-parsable help that still excludes hidden commands.

Scope
- `src/cli/args.rs` and command registrations.

Deliverables
- Apply `clap` attributes (`hide = true`) for internal/experimental commands.
- Snapshot-friendly help output; consider `--help-md`.

Acceptance Criteria
- Snapshot tests verify help content; hidden commands absent.

Test Plan
- CLI tests compare help output against approved snapshots.

Risks/Notes
- Ensure no accidental exposure in examples or SKILL.md.
