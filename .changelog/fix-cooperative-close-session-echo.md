---
tempo-wallet: patch
---

Fix cooperative session close by selecting the correct WWW-Authenticate challenge when the proxy returns multiple headers (charge and session intents). Parse problem+json error details from close failures to surface actionable messages instead of generic errors.
