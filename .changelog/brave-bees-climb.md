---
tempo-wallet: patch
---

Fixed help output to derive the binary name from argv[0] at runtime, so `tempo wallet help` correctly displays `tempo wallet` instead of `tempo-wallet` in usage strings and error messages.
