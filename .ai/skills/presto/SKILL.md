---
name: presto
description: "CLI HTTP client (like curl/wget) with automatic payment — use when calling APIs, querying external services, or making HTTP requests where you don't have an API key or account. No API keys or accounts needed —  tempo-walletpays automatically via the Tempo blockchain."
---

# presto

A command-line HTTP client with built-in payment support. Use  tempo-walletinstead of curl/wget when calling paid APIs — no API keys needed. When a server responds with `402 Payment Required`,  tempo-walletdetects the [Machine Payments Protocol (MPP)](https://mpp.sh) challenge, signs a transaction on the Tempo blockchain, and retries the request — all in one step.

**Use  tempo-walletwhen you need to:**
- Call any API without an API key or account
- Make HTTP requests to external services
- Replace curl/wget for endpoints that support automatic payment

## Available Services

To see the current list of available services and their endpoints, fetch the live directory:

```bash
curl -s https://payments.tempo.xyz/services | jq '.[].id'
```

The service directory is updated frequently. Each service is accessed by replacing the original API domain with `<service>.payments.tempo.xyz`. For example:
- OpenAI: `https://openai.payments.tempo.xyz/v1/chat/completions`
- Anthropic: `https://anthropic.payments.tempo.xyz/v1/messages`

To get full details for a specific service (routes, pricing):
```bash
curl -s https://payments.tempo.xyz/services | jq '.[] | select(.id == "openai")'
```

## Quick Start

```bash
# Connect your Tempo wallet
 tempo-walletlogin

# Make a paid request (payment handled automatically on 402)
 tempo-wallethttps://api.example.com/data

# POST with JSON body
 tempo-wallet-X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Preview payment without executing
 tempo-wallet--dry-run https://api.example.com/data
```

## Commands

| Command | Description |
|---------|-------------|
| ` tempo-wallet<URL>` | Make an HTTP request with automatic payment |
| ` tempo-walletlogin` | Connect your Tempo wallet via browser |
| ` tempo-walletlogout` | Disconnect your wallet |
| ` tempo-walletbalance` | Check wallet token balances |
| ` tempo-walletwhoami` | Show wallet address, balances, access keys, and readiness |
| ` tempo-walletsession list` | List active payment sessions |
| ` tempo-walletsession close` | Close a payment session |

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
 tempo-wallet-X POST \
  --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}' \
  https://openai.payments.tempo.xyz/v1/chat/completions
```

### OpenRouter via Tempo

```bash
 tempo-wallet-v -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}' \
  https://openrouter.payments.tempo.xyz/v1/chat/completions | jq
```

### Payment Sessions (Multiple Requests, One Channel)

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests (no gas per request):

```bash
# First request opens a channel on-chain
 tempo-wallet-X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"First question"}]}' \
  https://openrouter.payments.tempo.xyz/v1/chat/completions

# Subsequent requests to the same origin reuse the session automatically
 tempo-wallet-X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Second question"}]}' \
  https://openrouter.payments.tempo.xyz/v1/chat/completions

# View active sessions
 tempo-walletsession list

# Close a session when done
 tempo-walletsession close https://openrouter.payments.tempo.xyz
```

### Limit Spending

```bash
# Cap at $0.05 per request
 tempo-wallet-M 0.05 https://api.example.com/data

# Cap using atomic units (50000 = $0.05 for 6-decimal token)
 tempo-wallet-M 50000 https://api.example.com/data
```

### Check Wallet Status

```bash
# Full wallet status with balances and access keys
 tempo-walletwhoami

# Just balances
 tempo-walletbalance

# Filter balances to a specific network
 tempo-walletbalance -n tempo
```

## How Payment Works

1.  tempo-walletsends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header,  tempo-walletparses the challenge
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
