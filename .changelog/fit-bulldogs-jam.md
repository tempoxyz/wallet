---
tempo-common: patch
tempo-request: patch
tempo-wallet: patch
---

Track server-reported `Payment-Receipt.spent` for session state and use it as the close target instead of the payer-signed cumulative ceiling. This adds persisted `server_spent` support and updates request/session close flows so cooperative close settles the amount the server actually reports as spent.
