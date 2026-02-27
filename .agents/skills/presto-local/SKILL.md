---
name: presto
description: "CLI HTTP client with automatic payment — use when the user wants to call any external API or service without an API key or account, or when you need to access a capability but don't have a tool or API key for it. presto pays automatically via the Tempo blockchain. Use `presto -t services` to discover available services and endpoints."
---

# presto

A command-line HTTP client with built-in payment support. Use presto instead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`, presto detects the [Machine Payments Protocol (MPP)](https://mpp.dev) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use presto when you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Setup

Create a local wallet — no browser needed, keys stored in the OS keychain:

```bash
# Create a new local wallet (one-time setup)
presto wallets create

# Check wallet status
presto whoami
```

The wallet address printed by `presto whoami` is the **fundable address**.

### Funding the Wallet

**Always fund on mainnet** (the default). Do NOT use `--network tempo-moderato` unless the user explicitly asks for testnet.

```bash
# Fund the wallet — generates a deposit address and QR code for USDC on Base
presto wallets fund
```

The command prints a QR code and deposit address, then polls until funds are bridged to Tempo (up to 10 minutes). **Do NOT use `--no-wait`** — let the command block so you know when funding is complete. **Show the full command output to the user immediately** — they need to see the QR code and deposit address to send funds (they can expand collapsed output with ctrl+o).

## Agent Usage

Use `-t` to get TOON output (compact, token-efficient) or `-j` for JSON:

```bash
# Preferred pattern: TOON output (saves tokens)
presto -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Check wallet readiness before making requests
presto -t whoami
```

### Preflight Check

Before making paid requests, verify the wallet is ready:

```bash
presto -t whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, has a key, and is provisioned (or will auto-provision)
- `balance` — the wallet's USDC balance (top-level field)

If `ready` is `false` or the command errors:
1. Run `presto wallets create` — this is always the correct first step
2. If key is expired → run `presto keys create`
3. Do NOT run `presto login` — that opens a browser for passkey auth, which is not the local wallet flow

### whoami JSON Response Schema

```json
{
  "ready": true,
  "wallet": "0x1234...abcd",
  "wallet_type": "local",
  "symbol": "USDC",
  "balance": "10.50",
  "network": "tempo",
  "chain_id": 4217,
  "key": {
    "label": "local",
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

### keys JSON Response Schema

```json
{
  "keys": [
    {
      "label": "local",
      "address": "0xabcd...1234",
      "wallet_address": "0x1234...abcd",
      "wallet_type": "local",
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

## Available Services

Use `presto services` to discover available services, their endpoints, and pricing:

```bash
# List all available services
presto -t services

# Filter by category (ai, search, compute, blockchain, data, media, social, storage, web)
presto -t services --category ai

# Search by name, description, or tags
presto -t services --search <QUERY>

# Show full details for a service (endpoints, pricing, docs)
presto -t services info <SERVICE_ID>
```

Each service is accessed via its MPP service URL (shown in the `Service URL` column of `presto services`). When you don't know which service or endpoint to use, run `presto services info <id>` to see every endpoint with its HTTP method, path, pricing, and documentation links.

## Quick Start

```bash
# 1. Create a local wallet (one-time setup, no browser)
presto wallets create

# 2. Fund the wallet (shows QR code for USDC deposit on Base)
presto wallets fund

# 3. Discover available services
presto -t services

# 4. Make a paid request (payment handled automatically on 402)
presto -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Preview cost without paying
presto -t --dry-run -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

## Commands

| Command | Description |
|---------|-------------|
| `presto <URL>` | Make an HTTP request with automatic payment |
| `presto wallets create` | Create a new local wallet (OS keychain) |
| `presto wallets fund` | Fund your wallet (testnet faucet or mainnet bridge) |
| `presto whoami` | Show wallet address, balances, keys, and readiness |
| `presto keys list` | List all keys and their spending limits |
| `presto keys create` | Renew access key (fresh 30-day key) |
| `presto services` | List available MPP services |
| `presto services --category ai` | Filter services by category |
| `presto services --search <QUERY>` | Search services by name, description, or tags |
| `presto services info <ID>` | Show detailed info for a service (endpoints, pricing, docs) |
| `presto sessions` | Manage payment sessions (list, close — use `--help` for details) |

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-v` | Verbose output — shows payment flow details (intent, network, amount) (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-t, --toon-output` | TOON output — compact, token-efficient (recommended for agents) |
| `-j, --json-output` | JSON output |

## Query Options

These options apply when making HTTP requests (`presto <URL>`):

### Payment Options

| Option | Description |
|--------|-------------|
| `--dry-run` | Show what would be paid without executing |
| `--max-pay <AMOUNT>` | Hard cap on the maximum amount to pay |
| `--currency <ADDR\|SYMBOL>` | Currency for `--max-pay` |

### HTTP Options

| Option | Description |
|--------|-------------|
| `-X, --request <METHOD>` | Custom request method (GET, POST, PUT, DELETE, ...) |
| `-H, --header <HEADER>` | Add custom header (can be repeated) |
| `--json <JSON>` | Send JSON data with Content-Type header |
| `--toon <TOON>` | Send TOON data (decoded to JSON) with Content-Type header |
| `-d, --data <DATA>` | POST data (use `@filename` to read from file, `@-` for stdin) |
| `-L, --location` | Follow redirects (off by default) |
| `--max-redirs <N>` | Maximum number of redirects when `-L` is used |
| `-m, --timeout <SECONDS>` | Maximum time for the request |
| `--connect-timeout <SECONDS>` | Maximum time to establish TCP connection |
| `--offline` | Fail immediately without making any network requests |
| `-f, --fail` | Fail on HTTP errors (do not output body) |
| `--fail-with-body` | Fail on HTTP errors but still output the response body |
| `-k, --insecure` | Skip TLS certificate validation |
| `--compressed` | Request a compressed response |
| `--stream` | Stream response body as it arrives |
| `--sse` | Treat response as Server-Sent Events pass-through |
| `--sse-json` | Output SSE events as NDJSON |
| `--retries <N>` | Number of retries on transient network errors |

### Display Options

| Option | Description |
|--------|-------------|
| `-i, --include` | Include HTTP response headers in output |
| `-o, --output <FILE>` | Write output to file |

## Real-World Examples

### Making a Request

Use `presto services` to find the service URL and endpoint, then make the request:

```bash
# 1. Find the right service and endpoint
presto -t services info <SERVICE_ID>

# 2. Make the request (payment handled automatically on 402)
presto -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

### Payment Sessions (Multiple Requests, One Channel)

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests (no gas per request):

```bash
# First request opens a channel on-chain
presto -t -X POST --json '{"your":"payload"}' <SERVICE_URL>/<ENDPOINT_PATH>

# Subsequent requests to the same origin reuse the session automatically
presto -t -X POST --json '{"your":"payload"}' <SERVICE_URL>/<ENDPOINT_PATH>

# View active sessions
presto -t sessions list

# Close a session when done
presto -t sessions close <SERVICE_URL>

# Close all sessions
presto -t sessions close --all
```

### Check Wallet Status

```bash
# Full wallet status with balances and keys
presto -t whoami
```

### Environment Variable Override

For CI/CD or ephemeral use, skip wallet setup entirely:

```bash
# Use a private key directly (no keychain, no keys.toml)
PRESTO_PRIVATE_KEY=0xYOUR_HEX_KEY presto -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

## Error Recovery

Errors are printed to stderr in the format `Error: <message>` with specific exit codes.

### Exit Codes

| Code | Meaning | Agent Action |
|------|---------|--------------|
| 0 | Success | — |
| 1 | General error | Retry or report |
| 3 | Config error | Run `presto wallets create` |
| 4 | Network error | Check connectivity, retry |
| 5 | Payment failed | Check error message, retry |
| 6 | Insufficient funds | Run `presto wallets fund` or report to user |
| 8 | Auth/signing error | Run `presto keys create` |
| 10 | Timeout | Retry with longer `--timeout` |

### Common Errors and Fixes

| Error message contains | Action |
|------------------------|--------|
| `No wallet configured` | Run `presto wallets create`, then retry |
| `Run 'presto login'` | Run `presto wallets create`, then retry (ignore the login suggestion) |
| `Spending limit exceeded` | Report to user — key spending limit reached |
| `Insufficient balance` | Run `presto wallets fund` to add funds, or report to user |
| `Key is not provisioned` | Auto-provisions on first use; if persists, run `presto wallets create` |
| `Key expired` | Run `presto keys create` to renew |
| `Unknown network` | Check `-n` flag value |
| `401` RPC error | Set `PRESTO_RPC_URL` to an authenticated RPC endpoint |
| `timeout` | Retry with `-m <seconds>` |

When presto fails, read the error message — it tells you which command to run next. Run that command, then retry.

## How Payment Works

1. presto sends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header, presto parses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response
