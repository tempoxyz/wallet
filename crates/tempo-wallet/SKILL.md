---
name: tempo-wallet
description: |
  Manage your Tempo wallet — log in, check balances, fund, and manage keys. Use `tempo wallet -t login` to connect, `tempo wallet -t services` to browse available paid APIs, and `tempo request` to call them.

  TRIGGERS: wallet, balance, fund, login, spending limit, keys, whoami, check balance, wallet status, top up, deposit
---

# tempo wallet

Wallet identity and custody extension for the Tempo CLI. Manages authentication, key lifecycle, and funding. This binary handles all identity operations — for making HTTP requests with automatic payment, use `tempo request`.

**Use tempo wallet when you need to:**
- Connect or disconnect a wallet (`login` / `logout`)
- Check wallet readiness, balances, and key status (`whoami`)
- List keys and spending limits (`keys`)
- Fund your wallet (`fund`)

## Setup

If `tempo` is not found, install it first:

```bash
curl -fsSL https://tempo.xyz/install | bash
```

Then log in and check status:

```bash
tempo wallet -t login
tempo wallet -t whoami
```

## Agent Usage

Use `-t` for TOON output — compact and token-efficient. Output defaults to JSON automatically when stdout is piped (non-TTY), but `-t` saves more tokens.

```bash
# Check wallet readiness before making requests
tempo wallet -t whoami

# List keys with balances and spending limits
tempo wallet -t keys

# Preview funding without executing
tempo wallet -t fund --dry-run

# Discover command schema programmatically
tempo wallet -t --describe
```

### Preflight Check

Before making paid requests with `tempo request`, verify the wallet is ready:

```bash
tempo wallet -t whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, provisioned, and has a key
- `balance.total` — the wallet's total USDC balance
- `balance.available` — USDC available (not locked in sessions)

If `ready` is `false`, run `tempo wallet login` and retry.

### whoami JSON Response Schema

```json
{
  "ready": true,
  "wallet": "0x1234...abcd",
  "balance": {
    "total": 10.5,
    "locked": 1.0,
    "available": 9.5,
    "active_sessions": 1,
    "symbol": "USDC"
  },
  "key": {
    "address": "0xabcd...1234",
    "chain_id": 4217,
    "network": "tempo",
    "spending_limit": {
      "unlimited": false,
      "limit": 100.0,
      "remaining": 89.5,
      "spent": 10.5
    },
    "expires_at": "2026-03-26T00:00:00Z"
  }
}
```

### keys JSON Response Schema

```json
{
  "keys": [
    {
      "address": "0xabcd...1234",
      "chain_id": 4217,
      "network": "tempo",
      "wallet_address": "0x1234...abcd",
      "symbol": "USDC",
      "currency": "0x...",
      "balance": 10.5,
      "spending_limit": {
        "unlimited": false,
        "limit": 100.0,
        "remaining": 89.5,
        "spent": 10.5
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
| `tempo wallet keys` | List all keys with balance and spending limit details |
| `tempo wallet fund` | Fund your wallet (testnet faucet or mainnet bridge) |
| `tempo wallet fund --dry-run` | Preview funding action without executing |
| `tempo wallet sessions close --dry-run` | Preview what would be closed without executing |
| `tempo wallet mpp-sign` | Sign an MPP payment challenge |
| `tempo wallet --describe` | Emit command schema as JSON for agent introspection |

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-v` | Verbose output (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-t, --toon-output` | TOON output — compact, token-efficient (recommended for agents) |
| `-j, --json-output` | JSON output |
| `--describe` | Emit command schema as JSON (hidden) |

**Auto-detection:** When stdout is not a TTY (piped), output defaults to JSON automatically. Set `TEMPO_NO_AUTO_JSON=1` to disable.

## Error Recovery

Errors use structured `{ code, message, cause? }` JSON when output is JSON/TOON (including auto-detected). In text mode, errors print to stderr as `Error: <message>`.

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
| `Invalid hex input` | Fix the address/channel ID — likely a hallucinated URL parameter |
