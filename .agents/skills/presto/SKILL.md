---
name: presto
description: "CLI HTTP client with automatic payment — use when calling AI or LLM APIs, or any HTTP endpoint without an API key or account. Replaces curl/wget when the user has no API key.  tempo-walletpays automatically via the Tempo blockchain. Supports payment sessions, spending limits, and dry-run cost estimates."
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
| ` tempo-walletlogin` | Sign up or log in to your Tempo wallet |
| ` tempo-walletlogout` | Log out and disconnect your wallet |
| ` tempo-walletwhoami` | Show wallet address, balances, access keys, and readiness |
| ` tempo-walletsession list` | List active payment sessions |
| ` tempo-walletsession close [URL]` | Close a payment session (use `--all` to close all) |
| ` tempo-walletsession recover <URL>` | Recover a session from on-chain state |
| ` tempo-walletwallet create [--name]` | Create a local wallet (EOA stored in macOS Keychain) |
| ` tempo-walletwallet import [--name] [--stdin-key|--private-key]` | Import an existing private key as a local wallet |
| ` tempo-walletwallet delete --name <NAME> [--yes]` | Delete a local wallet |

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

These options apply when making HTTP requests (` tempo-wallet<URL>`):

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
# Full wallet status with balances and access keys
 tempo-walletwhoami
```

## Error Recovery

If  tempo-walletfails with "No wallet configured" or "Run ' tempo-walletlogin'", **automatically run ` tempo-walletlogin`** then retry the original request. Do NOT ask the user to run it themselves.

If  tempo-walletfails with a 401 RPC error, set `PRESTO_RPC_URL` to an authenticated RPC endpoint.

## How Payment Works

1.  tempo-walletsends the HTTP request normally
2. If the server returns `402 Payment Required` with a `WWW-Authenticate: Payment` header,  tempo-walletparses the challenge
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
