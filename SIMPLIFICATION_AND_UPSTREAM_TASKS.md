# mpp-rs Upstream Backlog (Post-Track A)

## Status

- Track A (presto simplification) is complete.
- Validation after Track A: `make check` passed.
- Current focus: upstream reusable SDK capabilities into `mpp-rs`.
- Priorities below were re-evaluated after Track A refactor.

Refactor-driven upstream gaps observed in presto:

- Client session streaming still imports `mpp::server::sse` parsing/types, so presto must enable the `mpp` `server` feature for client behavior.
- Retry/rejection classification after payment remains string/body driven in client integrations; typed errors are still missing at SDK boundary.
- Request orchestration is now modularized (`src/request.rs`), making SDK hook/policy APIs easier to adopt incrementally.

## Upstream Scope

Only upstream changes that are broadly reusable for Rust SDK consumers.

In scope:

- Generic 402 client orchestration hooks and typed retry/payment errors.
- Shared protocol utilities (receipt parsing extensions, amount formatting/conversion).
- Shared client/server streaming helpers for session voucher flows.
- Optional persistence/replay abstractions useful across multiple clients.

Out of scope:

- Presto-specific wallet/auth UX.
- Tempo app-specific policy and product behavior.

## Prioritized Upstream Tasks

| Priority | Task | Why this belongs upstream | Candidate mpp-rs areas |
| --- | --- | --- | --- |
| P0 | Move SSE event parsing/types into a shared client+server module | Track A revealed concrete coupling where client code imports `mpp::server::sse`; shared placement removes feature leakage and improves SDK layering. | refactor `src/server/sse.rs` into shared module + re-exports |
| P0 | Add RFC 9457 payment-rejection parsing helpers + typed client error for retry responses | After payment retries, clients need structured rejection data; today they parse strings/body manually. This is protocol-wide and should be first-class in client APIs. | `src/error.rs`, `src/client/error.rs`, `src/client/fetch.rs`, `src/client/middleware.rs`, `src/protocol/core/headers.rs` |
| P1 | Add a generic payment policy/hook API around 402 handling (`preflight`, `on_challenge`, `on_receipt`, `on_rejection`) | Clients currently reimplement orchestration to add constraints/analytics/dry-run behavior. A generic hook layer improves reuse across all consumers. | `src/client/fetch.rs`, `src/client/middleware.rs`, `src/client/provider.rs` |
| P1 | Extend receipt parsing to preserve optional/unknown fields (or provide raw decoded receipt + helpers) | Some servers include extra receipt metadata beyond core fields; preserving it avoids downstream custom base64/json parsing. | `src/protocol/core/challenge.rs`, `src/protocol/core/headers.rs` |
| P1 | Add a core amount utility module for decimal-string ↔ atomic conversions and formatting | Every client ends up reimplementing the same conversion/validation logic (max amount, display formatting). This is reusable protocol-adjacent utility. | new module under `src/protocol` or `src/utils.rs` |
| P1 | Provide a generic session stream driver primitive with voucher callback hooks and stall-retry strategy | Metered SSE flows require consistent handling of need-voucher/receipt events; a reusable driver prevents each client from building fragile custom loops. | new client utility module + shared SSE types |
| P2 | Introduce a storage trait for client session/channel state persistence | In-memory-only state limits real CLI usage; a generic persistence interface enables reusable file/db stores across SDK consumers. | `src/client/session_provider.rs`, `src/store.rs` |
| P2 | Add an optional retry request abstraction for non-cloneable request bodies | `send_with_payment` currently depends on clonable requests; a generic replay strategy broadens applicability beyond simple request builders. | `src/client/fetch.rs`, `src/client/middleware.rs` |

## Suggested Execution Order

1. Land `P0` SDK boundary fixes first (shared SSE module + typed retry rejection errors).
2. Add `P1` orchestration APIs (hooks/policy) on top of typed `P0` outcomes.
3. Add remaining `P1` protocol/streaming utilities (receipt extensions, amount utils, stream driver).
4. Add optional persistence/replay abstractions (`P2`) as follow-on enhancements.

## Handoff Notes

- For each upstream item accepted into `mpp-rs`, create a paired presto adoption task.
- Keep SDK APIs method-agnostic unless there is clear multi-consumer Tempo-only demand.
