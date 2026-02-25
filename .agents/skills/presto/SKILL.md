---
name: presto
description: "CLI HTTP client with automatic payment — use when calling AI or LLM APIs, or any HTTP endpoint without an API key or account. Replaces curl/wget when the user has no API key. presto pays automatically via the Tempo blockchain. Supports payment sessions, spending limits, and dry-run cost estimates."
---

# presto

A command-line HTTP client with built-in payment support. Use presto instead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`, presto detects the [Machine Payments Protocol (MPP)](https://mpp.sh) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use presto when you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Agent Usage

Set JSON as the default output format in config so you don't need `--output-format json` on every call:

```bash
# One-time setup: set default output format to JSON
# Add to ~/Library/Application Support/presto/config.toml (macOS)
# or ~/.config/presto/config.toml (Linux):
output_format = "json"
```

Then use `-q` (quiet) to suppress log messages:

```bash
# Preferred pattern: quiet, pipe through jq
presto -q -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq

# Check wallet readiness before making requests
presto -q whoami | jq '.ready'
```

### Preflight Check

Before making paid requests, verify the wallet is ready:

```bash
presto -q --output-format json whoami
```

Check these fields in the response:
- `ready` — `true` means the wallet is connected, provisioned, and has an access key
- `key.balance` — check that the token balance is sufficient

If `ready` is `false`, run `presto login` and retry.

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
  ]
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
presto login

# Make a paid LLM request (payment handled automatically on 402)
presto -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# Preview cost without paying
presto --dry-run -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions
```

## Commands

| Command | Description |
|---------|-------------|
| `presto <URL>` | Make an HTTP request with automatic payment |
| `presto login` | Sign up or log in to your Tempo wallet |
| `presto logout` | Log out and disconnect your wallet |
| `presto whoami` | Show wallet address, balances, access keys, and readiness |
| `presto session list` | List active payment sessions |
| `presto session list --all` | Show all channels: active, orphaned, and closing |
| `presto session list --orphaned` | Scan on-chain for orphaned channels (no local session) |
| `presto session list --closed` | Show channels pending finalization |
| `presto session close [URL]` | Close a payment session by URL or channel ID |
| `presto session close --all` | Close all active sessions and on-chain channels |
| `presto session close --orphaned` | Close only orphaned on-chain channels |
| `presto session close --closed` | Finalize channels pending close (grace period elapsed) |
| `presto wallet create [--name]` | Create a local wallet (EOA stored in macOS Keychain) |
| `presto wallet import [--name] [--stdin-key|--private-key]` | Import an existing private key as a local wallet |
| `presto wallet delete --name <NAME> [--yes]` | Delete a local wallet |
| `presto key` or `presto key list` | List all access keys and their spending limits |
| `presto key create [--name]` | Create a new access key for a local wallet (generates fresh 30-day key) |

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-n, --network <NETWORKS>` | Filter to specific networks (default: `tempo`) |
| `-v` | Verbose output — shows payment flow details (intent, network, amount) (use `-vv` for debug) |
| `-q, --quiet` | Suppress log messages (recommended for agents) |
| `--output-format json` | JSON output (recommended for agents) |
| `--color never` | Disable colored output |

## Query Options

These options apply when making HTTP requests (`presto <URL>`):

### Payment Options

| Option | Description |
|--------|-------------|
| `--dry-run` | Show what would be paid without executing |

### HTTP Options

| Option | Description |
|--------|-------------|
| `-X, --request <METHOD>` | Custom request method (GET, POST, etc.) |
| `-H, --header <HEADER>` | Add custom header (can be repeated) |
| `--json <JSON>` | Send JSON data with Content-Type header |
| `-d, --data <DATA>` | POST data (use `@filename` to read from file, `@-` for stdin) |
| `--no-redirect` | Disable following redirects |
| `-m, --timeout <SECONDS>` | Maximum time for the request |

### RPC Options

| Option | Description |
|--------|-------------|
| `-r, --rpc <URL>` | Override RPC URL for blockchain operations |

### Display Options

| Option | Description |
|--------|-------------|
| `-i, --include` | Include HTTP response headers in output |
| `-o, --output <FILE>` | Write output to file |

## Real-World Examples

### LLM API Request (Single Payment)

Each request is a separate on-chain transaction:

```bash
presto -X POST \
  --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}' \
  https://openai.mpp.tempo.xyz/v1/chat/completions
```

### OpenRouter via Tempo

```bash
presto -v -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq
```

### Image Generation via fal

```bash
presto -v -X POST \
  --json '{"prompt":"A golden retriever in a sunny park","image_size":"landscape_4_3","num_images":1}' \
  https://fal.mpp.tempo.xyz/fal-ai/flux/schnell
```

### Payment Sessions (Multiple Requests, One Channel)

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests (no gas per request):

```bash
# First request opens a channel on-chain
presto -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"First question"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# Subsequent requests to the same origin reuse the session automatically
presto -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Second question"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# View active sessions
presto session list

# Close a session when done
presto session close https://openrouter.mpp.tempo.xyz

# Close all sessions
presto session close --all
```

### Check Wallet Status

```bash
# Full wallet status with balances and access keys
presto whoami
```

## Error Recovery

Errors are printed to stderr in the format `Error: <message>` with specific exit codes.

### Exit Codes

| Code | Meaning | Agent Action |
|------|---------|--------------|
| 0 | Success | — |
| 1 | General error | Retry or report |
| 3 | Config error | Run `presto login` |
| 4 | Network error | Check connectivity, retry |
| 5 | Payment failed | Check error message, retry |
| 6 | Insufficient funds | Report to user — wallet needs funding |
| 8 | Auth/signing error | Run `presto login` |
| 10 | Timeout | Retry with longer `--timeout` |

### Common Errors and Fixes

| Error message contains | Action |
|------------------------|--------|
| `No wallet configured` | Run `presto login`, then retry |
| `Run 'presto login'` | Run `presto login`, then retry |
| `Spending limit exceeded` | Report to user — key spending limit reached |
| `Insufficient balance` | Report to user — wallet needs more funds |
| `Access key is not provisioned` | Run `presto login`, then retry |
| `Unknown network` | Check `-n` flag value |
| `401` RPC error | Set `PRESTO_RPC_URL` to an authenticated RPC endpoint |
| `timeout` | Retry with `-m <seconds>` |

When presto fails with a login-fixable error, **automatically run `presto login`** then retry the original request. Do NOT ask the user to run it themselves.

## How Payment Works

1. presto sends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header, presto parses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PRESTO_RPC_URL` | Override RPC endpoint (required for mainnet — see above) |
| `PRESTO_AUTH_URL` | Override auth server URL for login |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `PRESTO_PRIVATE_KEY` | Provide a private key directly for payment (bypasses wallet login and keychain; ephemeral) |
| `NO_COLOR` | Disable colored output |
