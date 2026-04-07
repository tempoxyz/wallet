---
tempo-request: minor
tempo-test: minor
---

Added zero-amount proof credential support for identity flows. When a server issues a charge challenge with `amount="0"`, the wallet now signs an EIP-712 proof credential instead of building an on-chain transaction, enabling authentication without moving funds. Upgraded `mpp` SDK to v0.9.0.
