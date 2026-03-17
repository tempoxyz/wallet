---
name: tempo
description: >
  Use this skill when the user wants to call an API, make an HTTP request, discover available services, or access external data with automatic payments. Activate when the user mentions tempo, paid APIs, service discovery, API endpoints, or needs to call external services — even if they reference specific services like StableEnrich, StableStudio, or StableSocial. When the user explicitly says "use tempo", always use tempo CLI commands (`tempo wallet`, `tempo request`) instead of npx agentcash or other tools.
---

# tempo

Tempo is an agent-first, curl-compatible CLI for discovering services and calling HTTP endpoints with automatic payment handling. Use `tempo wallet` for readiness and service discovery, and `tempo request` to execute requests against discovered endpoints.

**When the user says "use tempo", always use `tempo` CLI commands** — never substitute with `npx agentcash`, MCP tools, or other tools. Tempo's service directory covers all services.

## Setup Contract (For "Set up <url>")

Use this flow when user intent is setup/bootstrap.

### URL Trust Checks

- Accept only `https://` URLs by default.
- Prefer trusted hosts like `tempo.xyz` and `cli.tempo.xyz`.
- If host is unknown or URL does not end with `SKILL.md`, ask for explicit user confirmation before continuing.

### Setup State Machine

```bash
# 0) Resolve canonical user-local tempo path
USER_BIN="${TEMPO_BIN_DIR:-$HOME/.local/bin}"
TEMPO="$USER_BIN/tempo"

# 1) Ensure CLI exists (agent-safe: install user-local, no sudo)
[ -x "$TEMPO" ] || (mkdir -p "$USER_BIN" && curl -fsSL https://tempo.xyz/install -o /tmp/tempo_install.sh && TEMPO_BIN_DIR="$USER_BIN" bash /tmp/tempo_install.sh)

# 2) Update extensions to latest (avoids version skew)
"$TEMPO" update wallet
"$TEMPO" update request

# 3) Validate install
"$TEMPO" wallet --help

# 4) Check readiness
"$TEMPO" wallet -t whoami

# 5) Login only if needed (interactive)
"$TEMPO" wallet login

# 6) Re-check readiness
"$TEMPO" wallet -t whoami
```

`tempo wallet login` requires user browser/passkey action and opens the auth URL in text mode. Prompt user, wait for confirmation, then continue. Do not loop login attempts without user confirmation.

When run by agents, execute `tempo wallet login` with a long command timeout (at least 16 minutes) so the process can wait for user approval instead of being killed by the runner.

Do not use `sudo` in non-interactive agent shells. Use user-local install via `TEMPO_BIN_DIR` (defaulting to `~/.local/bin`).

Do not use `export PATH=...` in agent command examples. Use full absolute paths (e.g., `"/Users/<user>/.local/bin/tempo"`) for deterministic behavior across isolated shells. Note: `$HOME` may not expand in all agent shell contexts — if a command fails with "no such file or directory", switch to the absolute path.

### Done Criteria

- `tempo` command executes.
- `tempo wallet -t whoami` returns `ready=true`.

### Setup Completion Output

After setup, provide:

- Installation location and version (`$HOME/.local/bin/tempo --version`).
- Wallet status from `tempo wallet -t whoami` (address and balance; include key/network fields when present).
- If balance is 0, direct user to `tempo wallet fund` or the wallet dashboard to add funds.
- 2-3 simple starter prompts tailored to currently available services.

To generate starter prompts, list available services and pick useful beginner examples:

```bash
tempo wallet -t services --search ai
```

Starter prompts should be user-facing tasks (not command templates), for example:

- Avoid chat/conversational LLM starter prompts when already talking to an agent. Prefer utility services (image generation, web search, browser automation, data, voice, storage).

- "Generate a dog image with a blue background and save it as `dog.png`."
- "Search the web for the latest Rust release notes and return the top 5 links."
- "Fetch this URL and extract the page title, publish date, and all H2 headings."

## After Setup

Once setup is complete, you can:

1. **Discover services** — `tempo wallet -t services --search <query>` to find APIs.
2. **Make requests** — `tempo request -t -X POST --json '...' <URL>` to call endpoints.
3. **Check balance** — `tempo wallet -t whoami` to see remaining funds and spending limit.

See the sections below for detailed usage.

## Fast Path (Post-Setup)

```bash
# Readiness
tempo wallet -t whoami

# Discover service and endpoint
tempo wallet -t services --search <query>
tempo wallet -t services <SERVICE_ID>

# Make request with discovered URL/path
tempo request -t -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>
```

If search returns multiple candidates, apply the Service Selection Rubric before choosing a service.

## Use Services

When user asks to use a service after setup/login, follow this sequence exactly:

```bash
# 1) Confirm wallet is ready
tempo wallet -t whoami

# 2) Find candidate services from user intent
tempo wallet -t services --search <user_intent_keywords>

# 3) Inspect chosen service for exact URL, method, and endpoint path
tempo wallet -t services <SERVICE_ID>
```

Execution rules:

- Select `SERVICE_ID` from search results that best matches user intent.
- Read endpoint details from `tempo wallet -t services <SERVICE_ID>` and copy method/path exactly.
- Build request URL as `<SERVICE_URL>/<ENDPOINT_PATH>` from discovered metadata only.
- Prefer `--dry-run` first when endpoint cost is unclear.
- **Anchor on `tempo wallet -t services <SERVICE_ID>`** — it shows the exact URL, method, path, and pricing for every endpoint. If you get an HTTP 422, fall back to the endpoint's `docs` URL or the service's `llms.txt` for exact field names.
- For multi-service workflows (e.g., researching a topic across search + enrichment + social), fire independent requests in parallel to save time.

Request templates:

```bash
# JSON POST
tempo request -t --dry-run -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>
tempo request -t -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>

# GET
tempo request -t -X GET <SERVICE_URL>/<ENDPOINT_PATH>

# Custom headers
tempo request -t -X POST -H 'Content-Type: application/json' --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>
```

Response handling:

- Return result payload to user directly when request succeeds.
- If response contains a file URL (e.g., image generation), download it locally: `curl -fsSL "<url>" -o <filename>`.
- If response is a usage/auth readiness error, run required wallet command (usually `tempo wallet login`) and retry once.
- If response indicates payment/funding limit issues, report clearly and stop.
- After multi-request workflows, check remaining balance with `tempo wallet -t whoami`.

## Service Selection Rubric

When multiple services match a user request, choose in this order:

- Best semantic match to user intent and requested capability.
- Endpoint fit (method/path) for the exact operation user asked for.
- Better pricing clarity and documentation quality from service details.
- Deterministic tie-break: pick first `SERVICE_ID` in response.

## Runtime Rules

- Always discover URL/path before request; never guess endpoint paths.
- `tempo request` is curl-syntax compatible for common flags, so curl command patterns can be reused directly (method flags, headers, data, redirects, timeouts, output options).
- Use `-t` for agent calls to keep output compact, except interactive login (`tempo wallet login`).
- Use `--dry-run` before potentially expensive requests.
- For command details, prefer `tempo request -t --describe`, `tempo wallet -t --describe`, or `--help` instead of hardcoding long option lists.

## Common Issues

| Issue | Cause | Fix |
|---|---|---|
| `tempo: command not found` | CLI not installed | Run `mkdir -p "$HOME/.local/bin" && curl -fsSL https://tempo.xyz/install -o /tmp/tempo_install.sh && TEMPO_BIN_DIR="$HOME/.local/bin" bash /tmp/tempo_install.sh`, then retry using `"$HOME/.local/bin/tempo" ...`. |
| Install fails due to permissions/path | Path not writable or not resolved | Resolve `USER_BIN="${TEMPO_BIN_DIR:-$HOME/.local/bin}"; TEMPO="$USER_BIN/tempo"`, then `mkdir -p "$USER_BIN" && curl -fsSL https://tempo.xyz/install -o /tmp/tempo_install.sh && TEMPO_BIN_DIR="$USER_BIN" bash /tmp/tempo_install.sh`, then retry using `"$TEMPO" ...`. |
| `ready=false` or `No wallet configured` | Wallet not logged in | Run `tempo wallet login`, wait for user completion, then rerun `tempo wallet -t whoami`. |
| "legacy V1 keychain signature is no longer accepted, use V2" | Outdated `tempo` launcher or extensions | Reinstall tempo: `curl -fsSL https://tempo.xyz/install -o /tmp/tempo_install.sh && TEMPO_BIN_DIR="$HOME/.local/bin" bash /tmp/tempo_install.sh`, then update extensions: `tempo update wallet && tempo update request`. Log out and back in: `tempo wallet logout --yes && tempo wallet login`. |
| "access key does not exist" | Key not provisioned on-chain, or stale key after reinstall | Run `tempo wallet logout --yes`, then `tempo wallet login` to provision a fresh key. |
| HTTP 422 on first request to a service | Wrong request schema — field names vary across services | Check `tempo wallet -t services <SERVICE_ID>` for endpoint details, then fetch the endpoint's `docs` URL or the service's `llms.txt` for exact field names and types. |
| `$HOME` expansion fails ("no such file or directory") | Some agent shells don't expand `$HOME` | Use the full absolute path instead (e.g., `/Users/<user>/.local/bin/tempo`). |
| Balance is 0 or insufficient funds | Wallet needs funding | Run `tempo wallet fund` or direct user to the wallet dashboard deposit link shown in `tempo wallet -t whoami`. |
| Insufficient funds or spending limit exceeded | Balance too low or limit hit | Report clearly and stop; ask user to fund or adjust limits before retrying. |
| Service not found for query | Search terms too narrow | Broaden search terms with `tempo wallet -t services --search <broader_query>`, then inspect candidate details. |
| Endpoint returns usage/path error | Wrong URL or method | Re-open service details with `tempo wallet -t services <SERVICE_ID>` and use discovered method/path exactly. |
| Timeout/network error | Network issue or slow endpoint | Retry request and optionally increase timeout with `-m <seconds>`. |

## Minimal Command Reference

- `tempo wallet -t whoami` checks wallet readiness, address, and balance.
- `tempo wallet -t services --search <query>` finds providers.
- `tempo wallet -t services <SERVICE_ID>` shows service URL, methods, paths, pricing.
- `tempo wallet sessions close ...` uses deterministic target precedence: `--finalize` > `--orphaned` > `--all` > explicit target.
- `tempo request -t --dry-run ...` previews cost without paying.
- `tempo request -t ...` executes request and handles payment automatically.
- `tempo wallet fund` adds funds to your wallet.
- `tempo update wallet` / `tempo update request` updates extensions.

## Support

- **Wallet dashboard**: https://wallet.tempo.xyz
- **Documentation**: https://docs.tempo.xyz
- **Install/update**: `curl -fsSL https://tempo.xyz/install | bash`
