---
tempo-common: patch
tempo-request: patch
---

Add `--max-spend` / `TEMPO_MAX_SPEND` hard cap for cumulative session spend, enforced at challenge time, on session reuse, at channel open, and during streaming top-ups. Also reconcile on-chain channel state after cooperative-close 5xx failures before falling back to payer-side close, and fix session reuse to reject candidates from a different origin.
