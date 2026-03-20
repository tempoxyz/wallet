---
tempo-common: patch
tempo-request: patch
---

Fix session top-up which was completely broken due to five compounding bugs: ABI mismatch (topUp used uint128 instead of uint256 producing wrong function selector), voucher cumulative amount was incorrectly clamped to available balance preventing top-up from ever triggering, AmountExceedsDeposit problem type was not handled alongside InsufficientBalance, stale challenge echo was used for top-up requests causing server rejection, and missing requiredTopUp field in server response caused a hard failure instead of computing the value from local state. Also signals ChannelInvalidated when on-chain top-up fails so the caller can re-open a new channel.
