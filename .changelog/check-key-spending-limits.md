---
pget-cli: patch
---

Check key authorization spending limits before payment. When using keychain signing with enforced limits, the effective spending capacity is now min(wallet balance, remaining key limit). Also checks on-chain key provisioning status before building transactions and bumps gas fees when pending transactions are detected.
