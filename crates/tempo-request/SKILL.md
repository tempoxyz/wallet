---
name: tempo request
description: "CLI HTTP client with automatic payment — use when the user wants to call any external API or service without an API key or account, or when you need to access a capability but don't have a tool or API key for it. tempo request pays automatically via the Tempo blockchain. Use `tempo mpp -t services` to discover available services and endpoints."
---

# tempo request

A command-line HTTP client with built-in payment support. Use tempo request instead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`, tempo request detects the [Machine Payments Protocol (MPP)](https://mpp.dev) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use tempo request when you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Setup

Connect your Tempo wallet first:

```bash
# Sign up or log in (opens browser for passkey auth)
tempo wallet login

# Check wallet status
tempo wallet whoami
```

## Agent Usage

Use `-t` to get TOON output (compact, token-efficient) or `-j` for JSON:

```bash
# Preferred pattern: TOON output (saves tokens)
tempo request -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Check wallet readiness before making requests
tempo wallet -t whoami
```

### Preflight Check

Before making paid requests, verify the wallet is ready:

```bash
tempo wallet -t whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, provisioned, and has a key
- `balance` — the wallet's USDC balance (top-level field)

If `ready` is `false`, run `tempo wallet login` and retry.

## Available Services

Use `tempo mpp services` to discover available services, their endpoints, and pricing:

```bash
# List all available services
tempo mpp -t services

# Filter by category (ai, search, compute, blockchain, data, media, social, storage, web)
tempo mpp -t services --category ai

# Search by name, description, or tags
tempo mpp -t services --search <QUERY>

# Show full details for a service (endpoints, pricing, docs)
tempo mpp -t services info <SERVICE_ID>
```

Each service is accessed via its MPP service URL (shown in the `Service URL` column of `tempo mpp services`). When you don't know which service or endpoint to use, run `tempo mpp services info <id>` to see every endpoint with its HTTP method, path, pricing, and documentation links.

## Quick Start

```bash
# Connect your Tempo wallet
tempo wallet login

# Discover available services
tempo mpp -t services

# Make a paid request (payment handled automatically on 402)
tempo request -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Preview cost without paying
tempo request -t --dry-run -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-v` | Verbose output — shows payment flow details (intent, network, amount) (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-t, --toon-output` | TOON output — compact, token-efficient (recommended for agents) |
| `-j, --json-output` | JSON output |

## Request Options

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
| `-m, --timeout <SECONDS>` | Maximum time for the request |
| `--stream` | Stream response body as it arrives |
| `--sse` | Treat response as Server-Sent Events pass-through |
| `--sse-json` | Output SSE events as NDJSON |
| `--retries <N>` | Number of retries on transient network errors |
| `-o, --output <FILE>` | Write output to file |
| `-i, --include` | Include HTTP response headers in output |

Run `tempo request --help` for the full list of curl-compatible options (`-u`, `--proxy`, `--bearer`, `--compressed`, `--http2`, etc.).

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
| `Spending limit exceeded` | Report to user — key spending limit reached |
| `Insufficient balance` | Report to user — wallet needs more funds |
| `Key is not provisioned` | Run `tempo wallet login`, then retry |
| `Unknown network` | Check `-n` flag value |
| `401` RPC error | Set `TEMPO_RPC_URL` to an authenticated RPC endpoint |
| `timeout` | Retry with `-m <seconds>` |

When tempo request fails, read the error message — it tells you which command to run next. Run that command, then retry.

## How Payment Works

1. tempo request sends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header, tempo request parses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response
