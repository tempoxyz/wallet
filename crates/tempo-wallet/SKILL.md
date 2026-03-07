---
name: tempo wallet
description: "Wallet identity and custody — manage Tempo wallets, keys, and balances. Use `tempo wallet login` to connect, `tempo wallet whoami` to check readiness."
---

# tempo wallet

Wallet identity and custody extension for the Tempo CLI. Manages wallet creation, authentication, key lifecycle, and funding. This binary handles all identity operations — for making HTTP requests with automatic payment, use `tempo mpp`.

**Use tempo wallet when you need to:**
- Connect or disconnect a wallet (`login` / `logout`)
- Check wallet readiness, balances, and key status (`whoami`)
- List keys and spending limits (`keys list`)
- Create or manage wallets (`wallets create`, `wallets list`, `wallets fund`)

## Setup

```bash
# Install
curl -fsSL https://cli.tempo.xyz/install | bash

# Sign up or log in (opens browser for passkey auth)
tempo wallet login

# Check wallet status
tempo wallet whoami
```

## Agent Usage

Use `-t` for TOON output (compact, token-efficient) or `-j` for JSON:

```bash
# Check wallet readiness before making requests
tempo wallet -t whoami

# List keys with balances and spending limits
tempo wallet -t keys list
```

### Preflight Check

Before making paid requests with `tempo mpp`, verify the wallet is ready:

```bash
tempo wallet -t whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, provisioned, and has a key
- `balance` — the wallet's USDC balance (top-level field)

If `ready` is `false`, run `tempo wallet login` and retry.

### whoami JSON Response Schema

```json
{
  "ready": true,
  "wallet": "0x1234...abcd",
  "wallet_type": "passkey",
  "symbol": "USDC",
  "balance": "10.50",
  "network": "tempo",
  "chain_id": 4217,
  "key": {
    "label": "passkey",
    "address": "0xabcd...1234",
    "spending_limit": {
      "unlimited": false,
      "limit": "100.00",
      "remaining": "89.50",
      "spent": "10.50"
    },
    "expires_at": "2026-03-26T00:00:00Z"
  }
}
```

### keys list JSON Response Schema

```json
{
  "keys": [
    {
      "label": "passkey",
      "address": "0xabcd...1234",
      "wallet_address": "0x1234...abcd",
      "wallet_type": "passkey",
      "symbol": "USDC",
      "currency": "0x...",
      "balance": "10.50",
      "spending_limit": {
        "unlimited": false,
        "limit": "100.00",
        "remaining": "89.50",
        "spent": "10.50"
      },
      "expires_at": "2026-03-26T00:00:00Z"
    }
  ],
  "total": 1
}
```

## Commands

| Command | Description |
|---------|-------------|
| `tempo wallet login` | Sign up or log in to your Tempo wallet |
| `tempo wallet logout` | Log out and disconnect your wallet |
| `tempo wallet whoami` | Show wallet address, balances, keys, and readiness |
| `tempo wallet keys list` | List all keys with balance and spending limit details |
| `tempo wallet wallets list` | List configured wallets |
| `tempo wallet wallets create` | Create a new local wallet |
| `tempo wallet wallets fund` | Fund your wallet (testnet faucet or mainnet bridge) |

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-v` | Verbose output (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-t, --toon-output` | TOON output — compact, token-efficient (recommended for agents) |
| `-j, --json-output` | JSON output |

## Error Recovery

Errors are printed to stderr in the format `Error: <message>` with specific exit codes. With `-j` or `-t`, errors are output to stdout as structured `{ code, message, cause? }` objects.

### Exit Codes

| Code | Label | Meaning | Agent Action |
|------|-------|---------|--------------|
| 0 | — | Success | — |
| 1 | `E_GENERAL` | General error (IO, keychain, serialization) | Retry or report |
| 2 | `E_USAGE` | Invalid usage (bad args, config, keys, URLs) | Fix arguments or run `tempo wallet login` |
| 3 | `E_NETWORK` | Network error (connect, timeout, TLS, DNS) | Check connectivity, retry |
| 4 | `E_PAYMENT` | Payment failed (rejected, insufficient funds, spending limit) | Check error message, retry or fund wallet |

### Common Errors and Fixes

| Error message contains | Action |
|------------------------|--------|
| `No wallet configured` | Run `tempo wallet login`, then retry |
| `Run 'tempo wallet login'` | Run `tempo wallet login`, then retry |
| `Key is not provisioned` | Run `tempo wallet login`, then retry |
| `Unknown network` | Check `-n` flag value |
