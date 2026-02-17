---
name: presto
description: A wget-like CLI tool for making HTTP requests with automatic payment support
---

# presto

A command-line HTTP client with built-in payment support. When a server responds with `402 Payment Required`, presto detects the [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

## Quick Start

```bash
# Connect your Tempo wallet
presto login

# Make a paid request (payment handled automatically on 402)
presto https://api.example.com/data

# POST with JSON body
presto -X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Preview payment without executing
presto --dry-run https://api.example.com/data
```

## Commands

| Command | Description |
|---------|-------------|
| `presto <URL>` | Make an HTTP request with automatic payment |
| `presto login` | Connect your Tempo wallet via browser |
| `presto logout` | Disconnect your wallet |
| `presto balance` | Check wallet token balances |
| `presto whoami` | Show wallet address, balances, access keys, and readiness |
| `presto session list` | List active payment sessions |
| `presto session close` | Close a payment session |

## Query Options

### Payment Options

| Option | Description |
|--------|-------------|
| `-M, --max-amount <AMOUNT>` | Maximum amount willing to pay (e.g., `0.05` for dollars, or `50000` for atomic units) |
| `--dry-run` | Show what would be paid without executing |
| `-n, --network <NETWORKS>` | Filter to specific networks (e.g., `tempo`, `tempo-moderato`) |

### HTTP Options

| Option | Description |
|--------|-------------|
| `-X, --request <METHOD>` | Custom request method (GET, POST, etc.) |
| `-H, --header <HEADER>` | Add custom header (can be repeated) |
| `--json <JSON>` | Send JSON data with Content-Type header |
| `-d, --data <DATA>` | POST data (use `@filename` to read from file, `@-` for stdin) |
| `--no-redirect` | Disable following redirects |
| `-m, --timeout <SECONDS>` | Maximum time for the request |
| `-r, --rpc <URL>` | Override RPC URL |

### Display Options

| Option | Description |
|--------|-------------|
| `-v` | Verbose output (use `-vv` for debug) |
| `-q, --quiet` | Suppress log messages |
| `-i, --include` | Include HTTP response headers in output |
| `-o, --output <FILE>` | Write output to file |
| `--output-format json` | JSON output format |
| `--color never` | Disable colored output |

## Real-World Examples

### LLM API Request (Single Payment)

Each request is a separate on-chain transaction:

```bash
presto -X POST \
  --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}' \
  https://openai.payments.tempo.xyz/v1/chat/completions
```

### OpenRouter via Tempo

```bash
presto -v -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}' \
  https://openrouter.payments.tempo.xyz/v1/chat/completions | jq
```

### Payment Sessions (Multiple Requests, One Channel)

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests (no gas per request):

```bash
# First request opens a channel on-chain
presto -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"First question"}]}' \
  https://openrouter.payments.tempo.xyz/v1/chat/completions

# Subsequent requests to the same origin reuse the session automatically
presto -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Second question"}]}' \
  https://openrouter.payments.tempo.xyz/v1/chat/completions

# View active sessions
presto session list

# Close a session when done
presto session close https://openrouter.payments.tempo.xyz
```

### Limit Spending

```bash
# Cap at $0.05 per request
presto -M 0.05 https://api.example.com/data

# Cap using atomic units (50000 = $0.05 for 6-decimal token)
presto -M 50000 https://api.example.com/data
```

### Check Wallet Status

```bash
# Full wallet status with balances and access keys
presto whoami

# Just balances
presto balance

# Filter balances to a specific network
presto balance -n tempo
```

## How Payment Works

1. presto sends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header, presto parses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PRESTO_MAX_AMOUNT` | Default max payment amount |
| `PRESTO_NETWORK` | Default network filter |
| `PRESTO_RPC_URL` | Override RPC URL |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `NO_COLOR` | Disable colored output |
