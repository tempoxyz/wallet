---
tempo-wallet: minor
---

Added a `pay` subcommand for sending direct ERC-20 transfers to an address on Tempo. The command supports specifying recipient address, amount, and optional currency (defaulting to USDC on mainnet and pathUSD on testnet), and displays either a fetched explorer receipt or a locally formatted receipt on success.
