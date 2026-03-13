---
name: tempo-request
description: |
  HTTP client with automatic payment — call any API without API keys or accounts. Use when you need external data or capabilities not available locally. When a server responds with 402 Payment Required, tempo request handles the payment and retries automatically.

  TRIGGERS: call API, use an API, HTTP request, make a request, external data, no API key, paid API, "find info about", "look up", travel, search, scrape, generate image, generate video, social data, send email, validate email, phone call, research, use llm
---

# tempo request

A command-line HTTP client with built-in payment support. Use tempo request instead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`, tempo request detects the [Machine Payments Protocol (MPP)](https://mpp.dev) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use tempo request when you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Workflow

Follow these steps in order:

### 1. Check wallet readiness

```bash
tempo wallet -t whoami
```

Check `ready` is `true` and `balance` is sufficient. If `ready` is `false`, run `tempo wallet login` and retry.

### 2. Discover the right service and endpoint

**Always discover before guessing.** Service URLs and endpoint paths are not predictable — run discovery first.

```bash
# List all available services
tempo wallet -t services

# Search by category
tempo wallet -t services --search ai

# Search by name, description, or tags
tempo wallet -t services --search <QUERY>

# Show full details for a service (endpoints, pricing, docs)
tempo wallet -t services <SERVICE_ID>
```

Each service is accessed via its MPP service URL (shown in the `Service URL` column of `tempo wallet services`). Run `tempo wallet services <id>` to see every endpoint with its HTTP method, path, pricing, and documentation links.

### 3. Make the request

```bash
tempo request -t -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>
```

Payment is automatic: sends request, gets 402 challenge, signs payment, retries with credential, returns result.

## Important Rules

- **Always discover before guessing.** Service URLs include provider-specific paths. Run `tempo wallet -t services` and `tempo wallet -t services <id>` first.
- **Use `-t` for all agent calls.** TOON output is compact and token-efficient.
- **Use `--dry-run` before expensive operations.** Preview cost without paying.
- **Check balance before large operations.** Some calls can be expensive.

## Setup Assumption

If this skill is already loaded, treat it as already set up. Do not run install/login bootstrap preemptively.

Run requests directly and only trigger recovery when a command fails due to missing CLI or wallet auth:

- If `tempo` is missing, install with `curl -fsSL https://tempo.xyz/install | bash`, then retry.
- If wallet auth is missing, run `tempo wallet -t login`, wait for user completion, then retry.
- For first-call confidence, optionally run `tempo wallet -t whoami` and continue when `ready` is `true`.

## Agent Usage

Use `-t` for TOON output — compact and token-efficient. Output defaults to JSON automatically when stdout is piped (non-TTY), but `-t` saves more tokens.

```bash
# Preview cost without paying
tempo request -t --dry-run -X POST \
  --json '{"your":"payload"}' \
  <SERVICE_URL>/<ENDPOINT_PATH>

# Discover command schema programmatically
tempo request -t --describe
```

## Global Options

| Option | Description |
|--------|-------------|
| `-v` | Verbose output — shows payment flow details (intent, network, amount) (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-t, --toon-output` | TOON output — compact, token-efficient (recommended for agents) |
| `-j, --json-output` | JSON output |
| `--describe` | Emit command schema as JSON (hidden) |

**Auto-detection:** When stdout is not a TTY (piped), output defaults to JSON automatically. Set `TEMPO_NO_AUTO_JSON=1` to disable.

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
