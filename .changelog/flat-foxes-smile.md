---
tempo-request: patch
---

Harden paid session SSE handling for real-world providers by improving retry/stream behavior and session state synchronization across open, voucher, and streaming flows. This reduces false failures and makes strict payment processing more resilient to provider response quirks.
