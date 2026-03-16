# Spec Alignment Remaining Work

This file tracks only pending **integration** coverage work for the current branch.
Completed implementation and documentation tasks were intentionally removed.

## Ranked Integration Backlog

1. `IT-01` Full session open flow: 402 `intent="session"`, on-chain open path, persistence in `channels.db`, and successful authorized replay.
2. `IT-02` Session reuse on second request: no new open tx and `cumulative_amount` progression.
3. `IT-03a` Reuse guardrail: no reuse when `payee` differs.
4. `IT-03b` Reuse guardrail: no reuse when channel state is non-active (`Closing`).
5. `IT-04` SSE voucher flow: `payment-need-voucher` handling with HEAD-first transport and POST fallback.
6. `IT-05a` Open response receipt with `acceptedCumulative` is persisted.
7. `IT-05c` SSE `payment-receipt` event persists accepted cumulative amount.
8. `IT-05d` Persisted accepted cumulative is used as next reuse baseline.
9. `IT-06a` Cooperative close credential shape: `action`, `channelId`, `cumulativeAmount`, `signature`, DID `source`.
10. `IT-06b` On-chain close `requestClose -> Pending` transition with persisted countdown state.
11. `IT-06c` On-chain close `withdraw` after grace elapsed.
12. `IT-06d` On-chain close stays `Pending` when grace not elapsed and submits no tx.
13. `IT-07a` HTTP 410 `application/problem+json` with `.../session/channel-not-found` triggers retry/reopen logic.
14. `IT-07b` HTTP 410 `.../session/channel-finalized` triggers retry/reopen logic.
15. `IT-07c` HTTP 402 `.../session/insufficient-balance` follows structured recovery path and surfaces clean detail.
16. `IT-08a` Concurrent same-origin requests do not double-open; one request reuses or waits.
17. `IT-08b` Stale active marker is cleaned and channel becomes reusable.
18. `IT-08c` Concurrent writers do not orphan or overwrite channel persistence.
19. `IT-09` `sessions list` with seeded channel returns expected JSON shape (`sessions`, `channel_id`, `origin`, `deposit`, `spent`, `status`).
20. `IT-10` `sessions list` on empty DB returns `{"sessions": [], "total": 0}`.
21. `IT-11` `sessions list --state all` includes orphaned channels.
22. `IT-12` Malformed row emits degraded event (`channel store degraded`, `malformed_list_drops >= 1`).
23. `IT-13` `sessions close <channel_id>` exercises on-chain close path.
24. `IT-14` `sessions close --all` closes multiple channels.
25. `IT-15` `sessions sync` reconciles on-chain close state into local `Closing` with `grace_ready_at`.
26. `IT-16` Dry-run session challenge path: no tx, no DB write, cost output present.
27. `IT-17` Error-after-payment path preserves channel state for dispute and surfaces preserved-state message.
28. `IT-18` SSE voucher clamp behavior when `requiredCumulative > deposit`.
29. `IT-19` `payment-receipt` event cleanly terminates stream when applicable.
30. `IT-20` Stalled voucher resume path retries with exponential backoff up to configured max.
31. `IT-21` Top-up path when `requiredCumulative > deposit`: sends `action="topUp"`, updates local deposit, then resumes voucher flow.
32. `IT-22` Challenge request preserves wire field `recipient` (no local rename leakage into protocol parsing).
33. `IT-23` Open/voucher/close credentials keep spec field names (`action`, `channelId`, `cumulativeAmount`, `signature`, `transaction`).
34. `IT-24` Credential `source` is DID format `did:pkh:eip155:{chainId}:{address}`.
35. `IT-05b` Missing receipt on successful paid response logs warning-only and does not fail request.
36. `IT-05e` SSE initial header receipt persists before delayed stream receipt events.
37. `IT-25` Explicit HEAD-first voucher transport test with 405 fallback to POST and successful stream continuation.
38. `IT-26` Missing `methodDetails.chainId` defaults to Moderato (`42431`) instead of failing.
39. `IT-27` Malformed `requiredCumulative` (non-numeric) fails stream path deterministically.
40. `IT-28` Empty `requiredCumulative` fails stream path deterministically.
41. `IT-29` Voucher idempotency replay semantics: submitting same or lower `cumulativeAmount` after acceptance is handled as successful/idempotent behavior (no client-side regression or persistence rollback).
42. `IT-30` Paid request idempotency header behavior: `Idempotency-Key` is included on paid requests and retry behavior remains stable under duplicate processing responses.
43. `IT-31` Top-up challenge freshness recovery: stale/unknown challenge (`challenge-not-found`) triggers challenge refresh (`HEAD` 402 flow) before retrying top-up submission.
44. `IT-32` HEAD success path for voucher updates: when voucher `HEAD` returns success, client does not issue unnecessary fallback `POST`.
45. `IT-33` Failure-response receipt robustness: if a non-conformant server includes `Payment-Receipt` on 4xx/5xx, client does not treat it as successful paid state.
46. `IT-34` Alternate-endpoint voucher submission: stream on endpoint A and submit voucher/top-up on endpoint B protected by the same payment handler.
47. `IT-35` Sequential-session replacement safety: new session on same channel while prior stream is active does not corrupt local state and recovers cleanly.
48. `IT-36` `feePayer` mode coverage: end-to-end open/top-up behavior for `feePayer=true` versus `feePayer=false` (or omitted) paths.

## Ranked Unit Backlog

1. `UT-01` Add direct unit tests in `store/storage.rs` for save/load by `channel_id`, delete, and close-state update round trips.
2. `UT-02` Add `find_reusable_channel` selection tests in `store/storage.rs` (active-only filtering, param mismatch, most-recent `last_used_at`).
3. `UT-03` Add identity canonicalization tests at persistence boundaries (payer raw-address storage and payee/token canonical forms).
4. `UT-04` Add reuse-guard unit tests in `session/flow.rs` for non-active states and identity mismatch edge cases not covered by current cases.
5. `UT-05` Add receipt-application unit tests in `session/flow.rs` for accepted-cumulative monotonicity and missing/invalid receipt warning-only behavior.
6. `UT-06` Add SSE receipt/voucher parsing unit tests in `session/streaming.rs` for malformed `payment-need-voucher` payload fields and trailer/header receipt handling boundaries.
7. `UT-07` Add Problem Details classification unit tests in `tempo-common::payment::classify` for channel-invalidating vs non-invalidating session problem types.
8. `UT-08` Add close-operation unit tests in `close/onchain.rs` for requestClose vs withdraw branch selection based on mocked grace-period timing.
9. `UT-09` Add cooperative-close credential unit tests for DID derivation from stored payer address and spec field-name stability.
10. `UT-10` Add deterministic retry-backoff/jitter unit tests for session voucher/top-up retry policy calculations where possible.
