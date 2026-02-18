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
| `presto login` | Connect your Tempo wallet via browser (device code flow) |
| `presto logout` | Disconnect your wallet |
| `presto balance [ADDRESS]` | Check wallet token balances (optionally for a specific address) |
| `presto whoami` | Show wallet address, balances, access keys, and readiness |
| `presto session list` | List active payment sessions |
| `presto session close [URL]` | Close a payment session (use `--all` to close all) |

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `-n, --network <NETWORKS>` | Filter to specific networks (e.g., `tempo`, `tempo-moderato`) |
| `-v` | Verbose output (use `-vv` for debug) |
| `-q, --quiet` | Suppress log messages |
| `--output-format json` | JSON output format |
| `--color never` | Disable colored output |

## Query Options

These options apply when making HTTP requests (`presto <URL>`):

### Payment Options

| Option | Description |
|--------|-------------|
| `-M, --max-amount <AMOUNT>` | Maximum amount willing to pay (e.g., `0.05` for dollars, or `50000` for atomic units) |
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

## Error Recovery

If presto fails with "No wallet configured" or "Run 'presto login'", **automatically run `presto login`** then retry the original request. Do NOT ask the user to run it themselves.

If presto fails with a 401 RPC error, set `PRESTO_RPC_URL` to an authenticated RPC endpoint.

## How Payment Works

1. presto sends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header, presto parses the challenge
3. For **charge** intent: signs an on-chain payment transaction and retries with an `Authorization: Payment` credential
4. For **session** intent: opens a payment channel on-chain (first request), then uses off-chain vouchers for subsequent requests to the same origin
5. The server validates the credential and returns the response

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PRESTO_RPC_URL` | Override RPC URL (required for mainnet — see above) |
| `PRESTO_MAX_AMOUNT` | Default max payment amount |
| `PRESTO_NETWORK` | Default network filter |
| `PRESTO_AUTH_URL` | Override auth server URL for login |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `NO_COLOR` | Disable colored output |
