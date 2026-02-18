---
"presto": minor
---

Mainnet support: network is now derived from the 402 challenge's `chainId` instead of being hardcoded to testnet. Presto correctly pays on Tempo mainnet (chain 4217) or Moderato testnet (chain 42431) based on what the server requests.

Additional production hardening:
- `--max-amount` decimal conversion now uses lossless string-based parsing instead of f64 arithmetic, and respects the token's actual decimal places instead of hardcoding 6.
- Session store now uses file locking (`fs2`) to prevent concurrent CLI invocations from clobbering each other's session state.
- Removed duplicate `ChargeRequest` deserialization in the charge flow.
- Analytics network field defaults to `"unknown"` instead of empty string when detection fails.
