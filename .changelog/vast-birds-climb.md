---
tempo-common: patch
tempo-request: patch
tempo-wallet: patch
---

Adds an `accepted_cumulative` field to `ChannelRecord` to track the server-confirmed accepted amount separately from the payer-signed ceiling (`cumulative_amount`). Cooperative close now uses the accepted amount instead of the signing ceiling to avoid overcharging the payer, with a DB migration and monotonic update logic throughout the storage and request layers.
