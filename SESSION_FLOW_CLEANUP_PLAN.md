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

## Quality Bar

- Every phase lands with `make check` passing.
- Existing integration tests stay green.
- Each extracted unit has focused unit tests.
- Public docs and SKILL docs are updated in lockstep with behavior.

## Design Principles

1. **State-machine first**: model session flow as explicit stages, not one long control path.
2. **Pure-core + side-effects edges**: keep protocol decision logic pure where possible.
3. **Typed boundary errors**: keep source-carrying errors and deterministic user messages.
4. **Terminal safety by default**: sanitize all server-sourced message text before display.
5. **Spec traceability**: annotate where implementation intentionally deviates from draft defaults.

## Work Plan

## Phase 1: Split Session Flow Into Stages

Create internal stage modules and move logic out of `handle_session_request`:

- `challenge_stage`: parse/validate challenge and resolve chain/token/payee constraints.
- `deposit_stage`: compute deposit policy and clamp against wallet balance.
- `reuse_stage`: discover and validate reusable channels (local + on-chain checks).
- `open_stage`: perform open transaction + initial credential handshake.
- `request_stage`: execute paid request + receipt handling.

**Deliverable**: `handle_session_request` becomes orchestration glue that composes stage outputs.

## Phase 2: Normalize Shared Types

Introduce focused context structs so each stage receives only required inputs:

- `ResolvedSessionChallenge`
- `ChannelReuseCandidate`
- `OpenExecutionPlan`
- `PaidRequestResult`

Remove ad-hoc tuples and repeated field plumbing.

## Phase 3: Consolidate Error Mapping

Add a dedicated error-mapping module for session operations:

- centralize `PaymentRejected` reason extraction + sanitization
- centralize RFC-9457 problem handling for session-specific failure classes
- add table-driven tests for common server error payload forms

## Phase 4: Streaming Isolation

Move streaming voucher/top-up retry policy into a dedicated coordinator:

- isolate transport retry policy from protocol decisions
- keep timeout/backoff policy explicit and unit-tested
- expose concise hooks for integration tests

## Phase 5: Wallet Session Command Cohesion

In `tempo-wallet` session commands:

- extract shared target-selection and rendering helpers
- keep `list`, `sync`, and `close` behavior contracts centralized
- keep dry-run and execute targeting semantics tied to one selector path

## Test Strategy

1. Add unit tests for each new stage module.
2. Keep existing integration suites as behavior lock.
3. Add a focused regression suite for:
   - dry-run/execute selector parity
   - duplicate-finalize prevention
   - sanitized error output handling
4. Keep MPP boundary tests as protocol lock (`crates/tempo-request/tests/mpp_boundary.rs`).

## Documentation Plan

Update after each phase:

- `ARCHITECTURE.md` for new stage/module boundaries
- `README.md` command contract changes (if any)
- `crates/tempo-wallet/SKILL.md` and `crates/tempo-request/SKILL.md` for agent-facing usage

## Rollout Strategy

1. Land Phase 1+2 first with no behavior changes.
2. Land Phase 3 sanitization/error cleanup next.
3. Land Phase 4 streaming isolation behind unchanged public interfaces.
4. Land Phase 5 command cohesion and final doc pass.

Each phase should be independently mergeable and revertable.
