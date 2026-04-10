---
tempo-request: patch
---

Pass `signing_mode` to `TempoProvider` in the zero-amount charge path, matching the paid charge path. Without this, the provider defaults to Direct mode and ignores keychain configuration.
