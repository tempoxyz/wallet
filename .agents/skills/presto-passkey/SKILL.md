---
name: presto
description: "CLI HTTP client with automatic payment — use when the user wants to call any external API or service without an API key or account, or when you need to access a capability but don't have a tool or API key for it.  tempo-walletpays automatically via the Tempo blockchain. Use ` tempo-wallet-j services` to discover available services and endpoints."
---

# presto

A command-line HTTP client with built-in payment support. Use  tempo-walletinstead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`,  tempo-walletdetects the [Machine Payments Protocol (MPP)](https://mpp.dev) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use  tempo-walletwhen you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Setup

Connect your Tempo wallet via browser authentication:

```bash
# Sign up or log in (opens browser for passkey auth)
 tempo-walletlogin

# Check wallet status
 tempo-walletwhoami
```

## Agent Usage

Use `-j` to get JSON output:

```bash
# Preferred pattern: JSON output, pipe through jq
 tempo-wallet-j -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Check wallet readiness before making requests
 tempo-wallet-j whoami
```

### Preflight Check

Before making paid requests, verify the wallet is ready:

```bash
 tempo-wallet-j whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, provisioned, and has a key
- `balance` — the wallet's USDC balance (top-level field)

If `ready` is `false`, run ` tempo-walletlogin` and retry.

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

### keys JSON Response Schema

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

## Available Services

Use ` tempo-walletservices` to discover available services, their endpoints, and pricing:

```bash
# List all available services
 tempo-wallet-j services

# Filter by category (ai, search, compute, blockchain, data, media, social, storage, web)
 tempo-wallet-j services --category ai

# Search by name, description, or tags
 tempo-wallet-j services --search <QUERY>

# Show full details for a service (endpoints, pricing, docs)
 tempo-wallet-j services info <SERVICE_ID>
```

Each service is accessed via its MPP service URL (shown in the `Service URL` column of ` tempo-walletservices`). When you don't know which service or endpoint to use, run ` tempo-walletservices info <id>` to see every endpoint with its HTTP method, path, pricing, and documentation links.

## Quick Start

```bash
# Connect your Tempo wallet
 tempo-walletlogin

# Discover available services
 tempo-wallet-j services

# Make a paid request (payment handled automatically on 402)
 tempo-wallet-j -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Preview cost without paying
 tempo-wallet-j --dry-run -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

## Commands

| Command | Description |
|---------|-------------|
| ` tempo-wallet<URL>` | Make an HTTP request with automatic payment |
| ` tempo-walletlogin` | Sign up or log in to your Tempo wallet |
| ` tempo-walletlogout` | Log out and disconnect your wallet |
| ` tempo-walletwhoami` | Show wallet address, balances, keys, and readiness |
| ` tempo-walletservices` | List available MPP services |
| ` tempo-walletservices --category ai` | Filter services by category |
| ` tempo-walletservices --search <QUERY>` | Search services by name, description, or tags |
| ` tempo-walletservices info <ID>` | Show detailed info for a service (endpoints, pricing, docs) |
| ` tempo-walletsessions` | Manage payment sessions (list, close — use `--help` for details) |

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-v` | Verbose output — shows payment flow details (intent, network, amount) (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-j, --json-output` | JSON output (recommended for agents) |

## Query Options

These options apply when making HTTP requests (` tempo-wallet<URL>`):

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

Use ` tempo-walletservices` to find the service URL and endpoint, then make the request:

```bash
# 1. Find the right service and endpoint
 tempo-wallet-j services info <SERVICE_ID>

# 2. Make the request (payment handled automatically on 402)
 tempo-wallet-j -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

### Payment Sessions (Multiple Requests, One Channel)

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests (no gas per request):

```bash
# First request opens a channel on-chain
 tempo-wallet-j -X POST --json '{"your":"payload"}' <SERVICE_URL>/<ENDPOINT_PATH>

# Subsequent requests to the same origin reuse the session automatically
 tempo-wallet-j -X POST --json '{"your":"payload"}' <SERVICE_URL>/<ENDPOINT_PATH>

# View active sessions
 tempo-wallet-j sessions list

# Close a session when done
 tempo-wallet-j sessions close <SERVICE_URL>

# Close all sessions
 tempo-wallet-j sessions close --all
```

### Check Wallet Status

```bash
# Full wallet status with balances and keys
 tempo-wallet-j whoami
```

## Error Recovery

Errors are printed to stderr in the format `Error: <message>` with specific exit codes.

### Exit Codes

| Code | Meaning | Agent Action |
|------|---------|--------------|
| 0 | Success | — |
| 1 | General error | Retry or report |
| 3 | Config error | Run ` tempo-walletlogin` |
| 4 | Network error | Check connectivity, retry |
| 5 | Payment failed | Check error message, retry |
| 6 | Insufficient funds | Report to user — wallet needs funding |
| 8 | Auth/signing error | Run ` tempo-walletlogin` |
| 10 | Timeout | Retry with longer `--timeout` |

### Common Errors and Fixes

| Error message contains | Action |
|------------------------|--------|
| `No wallet configured` | Run ` tempo-walletlogin`, then retry |
| `Run ' tempo-walletlogin'` | Run ` tempo-walletlogin`, then retry |
| `Spending limit exceeded` | Report to user — key spending limit reached |
| `Insufficient balance` | Report to user — wallet needs more funds |
| `Key is not provisioned` | Run ` tempo-walletlogin`, then retry |
| `Unknown network` | Check `-n` flag value |
| `401` RPC error | Set `PRESTO_RPC_URL` to an authenticated RPC endpoint |
| `timeout` | Retry with `-m <seconds>` |

When  tempo-walletfails, read the error message — it tells you which command to run next. Run that command, then retry.

## How Payment Works

1.  tempo-walletsends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header,  tempo-walletparses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response
