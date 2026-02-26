# Open Source Readiness: Quality, Security, and Agent‑UX Backlog

This document tracks the incremental plan to harden presto for open source release. It focuses on test coverage, code quality, security, and an agent‑first CLI/UX. Tasks are grouped into phases with acceptance criteria to keep progress measurable.

Per‑task docs live under `docs/open-source/` and can be deleted individually as they are completed. See the task index at: `docs/open-source/INDEX.md`. The index is the canonical ordering; this overview groups work thematically by phase and may not reflect the fine-grained execution order.

Guiding principles:
- Maximize reliability and maintainability; no hidden panics or ad‑hoc unwraps.
- Agent‑first: stable JSON contracts, deterministic exit codes, clear stderr/stdout separation.
- Security by default: redaction, input validation, safe secrets handling, supply‑chain checks.
- No hidden commands are documented here; internal or experimental commands must remain hidden from help output and public docs.

Execution policy:
- Every PR must pass `make check` locally and in CI with zero issues.
- Add or update tests for every externally visible behavior.
- Favor small PRs that complete individual tasks below.

---

## Phase 1: Baseline + CI Hardening

1. Enforce lints and safety gates
   - Deliverables:
     - Add crate attributes where applicable: `#![forbid(unsafe_code)]`, `#![deny(warnings)]`.
     - Replace `unwrap`/`expect` in non‑test code with typed errors (`thiserror`/`anyhow`).
   - Acceptance:
     - `make check` succeeds with zero warnings.
     - `rg -n "[.]unwrap\(|[.]expect\(" src | wc -l` returns 0 (excluding tests/benches).

2. CI coverage, audit, deny, MSRV
   - Deliverables:
     - Add `cargo llvm-cov` job with HTML/LCOV artifacts and a minimum coverage threshold (≥85%).
     - Add `cargo audit` and `cargo deny` CI jobs.
     - CI matrix for stable and MSRV; consider beta for early warnings. Add `make coverage`.
   - Acceptance:
     - Coverage artifacts published; job fails below threshold.
     - `cargo audit` and `cargo deny` green.

3. Reproducible builds + version stamping
   - Deliverables:
     - Embed git commit and build info in `presto --version`.
     - Ensure release builds are stripped and deterministic where feasible.
   - Acceptance:
     - `presto --version` prints `presto x.y.z (commit <SHA>, <date>)`.

---

## Phase 2: Agent‑First CLI and Output

4. Standardize machine‑readable outputs
   - Deliverables:
     - Ensure every command supports `--json` with a stable schema.
     - JSON to stdout only; logs/diagnostics to stderr; add `--quiet`, `--no-color`, `--log-format json|text`.
   - Acceptance:
     - Integration tests assert JSON shape and stderr separation; help documents options.

5. Deterministic errors and exit codes
   - Deliverables:
     - Map all error paths to structured JSON: `{ code, message, hint, cause? }`.
     - Stable exit codes per error class; document the mapping.
   - Acceptance:
     - Integration tests intentionally trigger each error class and verify body + exit code.

6. Help UX without leaking hidden commands
   - Deliverables:
     - Use `clap` `hide = true` for internal/experimental commands.
     - Consider `--help-md` or machine‑parsable help that still excludes hidden commands.
   - Acceptance:
     - Snapshot tests compare help output; hidden commands absent.

7. Retries, timeouts, backoff flags
   - Deliverables:
     - Global flags: `--connect-timeout`, `--timeout`, `--retry N`, `--retry-backoff ms` with sane defaults.
   - Acceptance:
     - Mocked HTTP tests cover timeout and backoff behavior.

8. Streaming protocol contract
   - Deliverables:
     - Line‑delimited JSON events with `{ event, data, ts }` for streams; document the contract.
   - Acceptance:
     - Integration tests consume streams and verify event order/schema.

---

## Phase 3: Coverage Expansion

9. Config parsing and network resolution tests
   - Deliverables:
     - Unit tests for precedence: env > typed overrides > general table > default.
     - Property‑based tests (proptest) for arbitrary TOML maps.
   - Acceptance:
     - Coverage > 90% for config/network; proptests run quickly and deterministically.

10. Wallet credentials model and IO tests
    - Deliverables:
      - Tests for key selection priority (passkey > first with `key` > first lexicographic).
      - Enforce file perms `0600`; test rejects weaker perms (guarded on Unix).
      - Property tests for token limit parsing.
    - Acceptance:
      - Deterministic selection; IO rejects insecure perms.

11. Key authorization decode/validate/sign tests
    - Deliverables:
      - Valid/invalid RLP payloads, expiry boundaries, signature mismatch, expired auth.
    - Acceptance:
      - All branches covered; no panics.

12. HTTP client and 402→payment→response flow
    - Deliverables:
      - Mock server integration tests: 200 path; 402 one‑shot payment; 402 session payment; error paths.
      - Assert headers, redactions, retry semantics.
    - Acceptance:
      - Black‑box CLI tests validate end‑to‑end behavior.

13. Payment protocols
    - Deliverables:
      - Unit tests for fee math, channel state transitions, close semantics.
      - Integration tests with mocked MPP SDK responses including streaming.
    - Acceptance:
      - Deterministic channel close; precise error propagation.

14. Session list/close commands
    - Deliverables:
      - Black‑box tests for `list` JSON rendering and `close` idempotency.
    - Acceptance:
      - Re‑running `close` yields stable exit code and message.

15. Analytics/telemetry tests
    - Deliverables:
      - `PRESTO_NO_TELEMETRY` disables events.
      - Redaction tests scrub PII: headers, URLs, payloads.
    - Acceptance:
      - No network during tests; redaction unit tests cover all event fields.

16. Output rendering tests
    - Deliverables:
      - Golden tests for `--json` and `--text` outputs with snapshots.
    - Acceptance:
      - Stable snapshots; diffs require explicit approval.

---

## Phase 4: Security Depth

17. Redaction and logging guardrails
    - Deliverables:
      - Central redact helpers for `Authorization`, `X-API-Key`, bearer tokens, secrets.
      - Replace any direct logging of sensitive fields with redacted variants.
    - Acceptance:
      - Semgrep rule to catch sensitive logging; CI gate passes.

18. Secrets handling improvements
    - Deliverables:
      - Evaluate `zeroize` for in‑memory key material where feasible.
      - Ensure keychain integration does not log secrets; typed errors for failures.
    - Acceptance:
      - Keys zeroized on drop; tests confirm no accidental prints.

19. Input validation and URL safety
    - Deliverables:
      - Validate URL schemes (`http`/`https`), disallow `file:`, `data:`, etc.
      - By default, block localhost unless `--allow-localhost`.
      - Disallow header injection via `\r\n`.
    - Acceptance:
      - Unit tests for invalid URLs and header names/values.

20. Fuzzing targets with `cargo-fuzz`
    - Deliverables:
      - Targets: config TOML parse, credentials TOML parse, RLP decode, channel transitions, HTTP header construction.
      - Nightly CI smoke run with a small time budget.
    - Acceptance:
      - Fuzz harnesses compile and run locally; CI smoke runs succeed.

21. SAST: Semgrep and CodeQL
    - Deliverables:
      - Semgrep ruleset including custom rules for redaction and panic prohibition.
      - CodeQL workflow for Rust; baseline triage.
    - Acceptance:
      - CI passes with no high‑severity findings; new findings require triage.

22. Supply chain checks
    - Deliverables:
      - `cargo audit` and `cargo deny` enforced (Phase 1).
      - Consider `cargo-vet`; patch risky transitive deps if needed.
    - Acceptance:
      - CI blocks on deny list or vulnerabilities.

---

## Phase 5: Documentation, Examples, Release

23. SKILL.md for AI agents (revamp)
    - Deliverables:
      - “Agent Recipes”: plain HTTP with auto‑402 handling; OpenRouter “ask the oracle”; session payments; streaming LD‑JSON; error handling via exit codes; rate limit/retry flags; redaction guidance.
      - Explicit note: hidden/experimental commands are intentionally excluded.
    - Acceptance:
      - Internal agent harness validates recipes end‑to‑end.

24. README and CLI reference
    - Deliverables:
      - Project overview, safety notes, telemetry opt‑out, exit code table, coverage badge, build/install, quickstart.
      - “Stable Interface Contract” for agents: JSON shapes and deprecation policy.
    - Acceptance:
      - Fresh install + quickstart works exactly as documented.

25. SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md
    - Deliverables:
      - Responsible disclosure process; contribution flow; gates (`make check`, coverage threshold).
    - Acceptance:
      - Linked from README; CI validates presence.

26. Examples for agents
    - Deliverables:
      - Minimal bash and Python examples for streaming LD‑JSON, retry loops, stdin payloads (`--json @-`).
    - Acceptance:
      - CI runs examples in a mock environment as smoke tests.

27. Release checklist and changelog
    - Deliverables:
      - `RELEASE.md` and `CHANGELOG.md` process; semantic versioning; interface change notes.
    - Acceptance:
      - First release executed via checklist without surprises.

---

## Phase 6 (Post‑GA Niceties)

28. CLI introspection gated by env
    - Deliverables:
      - `PRESTO_AGENT_INTROSPECT=1 presto --introspect` prints JSON schema of commands/flags, not exposed in normal help.
    - Acceptance:
      - Not documented in end‑user docs; only referenced at a high level.

29. Offline mode and deterministic mocks
    - Deliverables:
      - `--offline` flag; tests and CI paths that force mocks and prevent network.
    - Acceptance:
      - CI uses offline mode paths reliably.

30. Telemetry schema doc and sampling
    - Deliverables:
      - Public event schema; confirm no payload/body data is sent; optional sampling control.
    - Acceptance:
      - Telemetry tests assert schema adherence.

---

## Cross‑Cutting Quality Bars

- New code includes unit/integration tests as applicable.
- Externally visible behavior is covered by golden/snapshot tests where useful.
- Logs never leak secrets; redaction is centrally tested.
- No interactive prompts without explicit `--interactive`.
- `make check` and coverage gates pass locally and in CI.
