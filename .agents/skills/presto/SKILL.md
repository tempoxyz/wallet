---
name: presto
description: "CLI HTTP client with automatic payment — use when calling AI or LLM APIs, or any HTTP endpoint without an API key or account. Replaces curl/wget when the user has no API key.  tempo-walletpays automatically via the Tempo blockchain. Supports payment sessions, spending limits, and dry-run cost estimates."
---

# presto

A command-line HTTP client with built-in payment support. Use  tempo-walletinstead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`,  tempo-walletdetects the [Machine Payments Protocol (MPP)](https://mpp.dev) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use  tempo-walletwhen you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Agent Usage

Use `-j` to get JSON output:

```bash
# Preferred pattern: JSON output, pipe through jq
 tempo-wallet-j -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq

# Check wallet readiness before making requests
 tempo-wallet-j whoami | jq '.ready'
```

### Preflight Check

Before making paid requests, verify the wallet is ready:

```bash
 tempo-wallet-j whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, provisioned, and has a key
- `key.balance` — check that the token balance is sufficient

If `ready` is `false`, run ` tempo-walletlogin` and retry.

### whoami JSON Response Schema

```json
{
  "ready": true,
  "wallet": "0x1234...abcd",
  "wallet_type": "passkey",
  "network": "tempo",
  "chain_id": 4217,
  "key": {
    "label": "passkey-default",
    "address": "0xabcd...1234",
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
}
```

### keys JSON Response Schema

```json
{
  "keys": [
    {
      "label": "passkey-default",
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

To see the current list of available services and their endpoints, fetch the live directory:

```bash
curl -s https://mpp.tempo.xyz/services | jq '.[].id'
```

The service directory is updated frequently. Each service is accessed by replacing the original API domain with `<service>.mpp.tempo.xyz`. For example:
- OpenAI: `https://openai.mpp.tempo.xyz/v1/chat/completions`
- Anthropic: `https://anthropic.mpp.tempo.xyz/v1/messages`
- fal (image gen): `https://fal.mpp.tempo.xyz/fal-ai/flux/schnell`

To get full details for a specific service (routes, pricing):
```bash
curl -s https://mpp.tempo.xyz/services | jq '.[] | select(.id == "openai")'
```

## Quick Start

```bash
# Connect your Tempo wallet
 tempo-walletlogin

# Make a paid LLM request (payment handled automatically on 402)
 tempo-wallet-X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# Preview cost without paying
 tempo-wallet--dry-run -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions
```

## Commands

| Command | Description |
|---------|-------------|
| ` tempo-wallet<URL>` | Make an HTTP request with automatic payment |
| ` tempo-walletlogin` | Sign up or log in to your Tempo wallet |
| ` tempo-walletlogout` | Log out and disconnect your wallet |
| ` tempo-walletwhoami` | Show wallet address, balances, keys, and readiness |
| ` tempo-walletsession list` | List active payment sessions |
| ` tempo-walletsession list --all` | Show all channels: active, orphaned, and closing |
| ` tempo-walletsession list --orphaned` | Scan on-chain for orphaned channels (no local session) |
| ` tempo-walletsession list --closed` | Show channels pending finalization |
| ` tempo-walletsession close [URL]` | Close a payment session by URL or channel ID |
| ` tempo-walletsession close --all` | Close all active sessions and on-chain channels |
| ` tempo-walletsession close --orphaned` | Close only orphaned on-chain channels |
| ` tempo-walletsession close --closed` | Finalize channels pending close (grace period elapsed) |
| ` tempo-walletkey` or ` tempo-walletkey list` | List all keys and their spending limits |

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

### LLM API Request (Single Payment)

Each request is a separate on-chain transaction:

```bash
 tempo-wallet-X POST \
  --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}' \
  https://openai.mpp.tempo.xyz/v1/chat/completions
```

### OpenRouter via Tempo

```bash
 tempo-wallet-v -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq
```

### Image Generation via fal

```bash
 tempo-wallet-v -X POST \
  --json '{"prompt":"A golden retriever in a sunny park","image_size":"landscape_4_3","num_images":1}' \
  https://fal.mpp.tempo.xyz/fal-ai/flux/schnell
```

### Payment Sessions (Multiple Requests, One Channel)

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests (no gas per request):

```bash
# First request opens a channel on-chain
 tempo-wallet-X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"First question"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# Subsequent requests to the same origin reuse the session automatically
 tempo-wallet-X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Second question"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# View active sessions
 tempo-walletsession list

# Close a session when done
 tempo-walletsession close https://openrouter.mpp.tempo.xyz

# Close all sessions
 tempo-walletsession close --all
```

### Check Wallet Status

```bash
# Full wallet status with balances and keys
 tempo-walletwhoami
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

When  tempo-walletfails with a login-fixable error, **automatically run ` tempo-walletlogin`** then retry the original request. Do NOT ask the user to run it themselves.

## How Payment Works

1.  tempo-walletsends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header,  tempo-walletparses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response
