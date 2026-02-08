---
pget-cli: patch
---

Fix payment swap logic to distinguish spending limit vs balance bottlenecks. When a key's spending limit is too low, error immediately instead of attempting a swap that would fail on-chain with `SpendingLimitExceeded`. When balance is the bottleneck, also check the key's spending limit on swap source tokens before selecting them.
