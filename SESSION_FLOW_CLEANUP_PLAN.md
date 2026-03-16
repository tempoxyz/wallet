# Session Flow Cleanup Plan

## Goal

Refactor session payment execution into a reference-quality implementation that is easier to audit, easier to extend, and easier for MPP integrators to learn from, while preserving existing wire behavior and CLI contracts.

## Scope

This plan targets the core session execution path and related close/sync orchestration:

- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/open.rs`
- `crates/tempo-request/src/payment/session/persist.rs`
- `crates/tempo-wallet/src/commands/sessions/{list,close,sync}.rs`

## Non-Goals

- Protocol semantics changes (MPP wire format, challenge/receipt requirements)
- New CLI flags or changed output schemas
- Major persistence-format churn

## Compatibility Posture

This cleanup does **not** carry legacy/backward-compatibility obligations for internal structure or deprecated implementation patterns.

- Prefer the clearest reference-quality design over preserving historical internal abstractions.
- Remove obsolete/refactor-hostile pathways when replacement behavior is covered by tests/docs.
- Do not add compatibility shims solely to preserve legacy implementation details.

## Design Principles

1. **State-machine first**: model session flow as explicit stages, not one long control path.
2. **Pure-core + side-effects edges**: keep protocol decision logic pure where possible.
3. **Typed boundary errors**: keep source-carrying errors and deterministic user messages.
4. **Terminal safety by default**: sanitize all server-sourced message text before display.
5. **Spec traceability**: annotate where implementation intentionally deviates from draft defaults.

## Behavior Invariants (Must Hold Every Phase)

1. HTTP wire behavior is unchanged for equivalent inputs (headers, retry semantics, receipt handling expectations).
2. CLI contracts are unchanged (flags, output schema, exit codes, and side-effect timing).
3. Session lock semantics remain origin-serialized for paid request lifecycle.
4. Duplicate-close/finalize protections remain intact.
5. Server-provided human-readable text is terminal-sanitized before becoming `PaymentRejected` output.

## Cross-Phase Quality Gates

- [ ] `make check` passes.
- [ ] Existing integration suites pass.
- [ ] Existing MPP boundary tests pass (`crates/tempo-request/tests/mpp_boundary.rs`).
- [ ] New/changed units include focused unit tests (happy path + failure path).
- [ ] No phase introduces undocumented environment variables or behavior toggles.
- [ ] `ARCHITECTURE.md` and crate SKILL docs are updated if boundaries/contracts changed.

## Phase 0: Baseline Locks (Pre-Refactor)

### Checklist

- [ ] Capture current behavior locks for session open/reuse/request/streaming/close in tests before moving code.
- [ ] Add explicit regression tests for terminal sanitization in all session `PaymentRejected` pathways.
- [ ] Add selector-parity and duplicate-finalize tests as non-regression anchors (if not already exhaustive).

### Acceptance Criteria

- [ ] A failing change to any of the following is detected by tests: session reuse eligibility, open retry policy, stream voucher retry/backoff progression, close target selection parity.
- [ ] A fixture with control characters/ANSI escape content in server error payloads cannot reach terminal output unsanitized.
- [ ] Baseline tests are deterministic (no wall-clock sleeps beyond bounded test controls).

## Phase 1: Split Session Flow Into Stages

Create internal stage modules and move logic out of `handle_session_request`:

- `challenge_stage`: parse/validate challenge and resolve chain/token/payee constraints.
- `deposit_stage`: compute deposit policy and clamp against wallet balance.
- `reuse_stage`: discover and validate reusable channels (local + on-chain checks).
- `open_stage`: perform open transaction + initial credential handshake.
- `request_stage`: execute paid request + receipt handling.

### Checklist

- [ ] Introduce stage modules with narrow public functions and private helpers.
- [ ] Keep side-effecting operations (disk/network/console) at stage boundaries, not interleaved with pure decisions.
- [ ] Convert `handle_session_request` into orchestration glue with explicit stage transitions.
- [ ] Add stage-level tests proving identical decisions for representative fixtures.

### Acceptance Criteria

- [ ] `handle_session_request` no longer contains multi-concern business logic; each stage is independently testable.
- [ ] No behavior drift in end-to-end tests for: fresh open, reusable channel success, invalidated-channel fallback, reuse-failure preservation.
- [ ] Stage transition errors preserve typed `TempoError` chains.

## Phase 2: Normalize Shared Types

Introduce focused context structs so each stage receives only required inputs:

- `ResolvedSessionChallenge`
- `ChannelReuseCandidate`
- `OpenExecutionPlan`
- `PaidRequestResult`

### Checklist

- [ ] Replace ad-hoc tuples and repeated argument threading with named structs.
- [ ] Ensure each struct captures invariants (e.g., parsed identifiers, normalized origin, bounded deposit).
- [ ] Ensure types are minimal (no “god context” that reintroduces hidden coupling).
- [ ] Add unit tests for constructor/validation logic on shared types.

### Acceptance Criteria

- [ ] No tuple return values remain across session orchestration boundaries.
- [ ] Every shared struct has at least one invariant test and one serialization/format test if persisted/rendered.
- [ ] Static analysis (clippy + tests) shows no new `unwrap`-style panic risk in refactored paths.

## Phase 3: Consolidate Error Mapping

Add a dedicated error-mapping module for session operations:

- centralize `PaymentRejected` reason extraction + sanitization
- centralize RFC-9457 problem handling for session-specific failure classes
- add table-driven tests for common server error payload forms

### Checklist

- [ ] Create one mapping surface for session HTTP problem/error payloads.
- [ ] Route `flow.rs`, `open.rs`, `streaming.rs`, and close/sync session HTTP errors through it.
- [ ] Encode sanitization rules in helpers, not call-site conventions.
- [ ] Add table-driven tests for JSON problem, JSON `error`, plaintext, oversized body, and malformed body.

### Acceptance Criteria

- [ ] No duplicate free-form reason extraction logic remains in scoped files.
- [ ] All server-derived `PaymentRejected.reason` strings are sanitized and length-bounded.
- [ ] Problem classification behavior remains unchanged for known session problem types.

## Phase 4: Streaming Isolation

Move streaming voucher/top-up retry policy into a dedicated coordinator:

- isolate transport retry policy from protocol decisions
- keep timeout/backoff policy explicit and unit-tested
- expose concise hooks for integration tests

### Checklist

- [ ] Extract coordinator managing pending voucher state, retry counter, and timeout transitions.
- [ ] Keep SSE parsing/printing separate from voucher transport policy.
- [ ] Keep `HEAD`-first with `POST` fallback behavior explicit and covered.
- [ ] Keep idempotency-key handling deterministic across retries and fallback paths.

### Acceptance Criteria

- [ ] Retry/backoff progression is unit-tested (base stall, exponential increase, capped normal timeout, retry exhaustion).
- [ ] Transport fallback matrix is covered (`HEAD` unsupported, transport error, problem response, success receipt).
- [ ] No duplicate voucher submission occurs after terminal receipt/finalization signals.

## Phase 5: Wallet Session Command Cohesion

In `tempo-wallet` session commands:

- extract shared target-selection and rendering helpers
- keep `list`, `sync`, and `close` behavior contracts centralized
- keep dry-run and execute targeting semantics tied to one selector path

### Checklist

- [ ] Consolidate selector semantics into shared helpers consumed by both dry-run and execute.
- [ ] Keep structured output free of stray informational stderr noise.
- [ ] Ensure orphaned discovery persistence and render rules stay consistent between list/sync/close.
- [ ] Add contract tests for command output shape and selector precedence.

### Acceptance Criteria

- [ ] Dry-run and execute target sets are identical for equivalent flag combinations.
- [ ] Channel-ID, URL, `--all`, `--orphaned`, and `--finalize` precedence is single-path and regression-tested.
- [ ] No duplicate finalize-attempt side effects occur in mixed local/orphaned flows.

## Documentation Plan

Update after each phase when behavior/module boundaries shift:

- `ARCHITECTURE.md` for stage/module boundaries and session retry model.
- Root and crate `README` files only if user-facing command contract text changed.
- Root `SKILL.md`, `crates/tempo-wallet/SKILL.md`, and `crates/tempo-request/SKILL.md` for agent-facing invocation and contract details.

## Execution Governance

### Single-PR Phase Grouping

This cleanup will be delivered in the same active PR. Treat phases as internal review checkpoints inside one PR.

Before implementation starts:

- [ ] Group commits by phase so each phase can be reviewed and reverted independently.
- [ ] Keep phase ordering explicit in commit messages/checkpoints (0 → 5).

### Rollback Triggers

Any one trigger below requires immediate rollback or revert of the active phase commit group inside the PR:

- [ ] Behavior lock test regression in session reuse/open/request/streaming/close semantics.
- [ ] Any unsanitized server-provided text reaching terminal-facing `PaymentRejected` output.
- [ ] Selector parity regression between dry-run and execute close targeting.
- [ ] Duplicate finalize-attempt side effect in mixed local/orphaned flows.
- [ ] Newly introduced non-deterministic test that cannot be bounded within existing CI tolerances.

### Final Sign-Off Checklist

Before declaring the cleanup complete:

- [ ] All phase acceptance criteria are checked and linked to commits/checkpoints in this PR.
- [ ] `ARCHITECTURE.md` reflects final stage and coordinator boundaries.
- [ ] Root and crate `README` files reflect any user-facing contract changes (or explicit note that none occurred).
- [ ] Root `SKILL.md`, `crates/tempo-wallet/SKILL.md`, and `crates/tempo-request/SKILL.md` reflect final command/flow contracts.
- [ ] `make check` passes on the final integration branch.

## Rollout Strategy

1. Complete Phase 0 first (test locks), then Phase 1 and Phase 2 with no intended behavior changes.
2. Complete Phase 3 next so sanitization and mapping are centralized before deeper streaming movement.
3. Complete Phase 4 behind unchanged public interfaces and previously established behavior locks.
4. Complete Phase 5 and final docs pass.

Within this single PR, each phase should remain independently reviewable, independently revertable (via commit grouping), and should not rely on unfinished behavior in later phases.
