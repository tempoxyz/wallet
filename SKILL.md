---
name: tempo
description: |
  Agent-first, curl-compatible CLI for discovering services and calling HTTP APIs with built-in payments. Use `tempo wallet` to check readiness and discover service endpoints, then use `tempo request` to execute calls. If an endpoint returns `402 Payment Required`, tempo handles MPP payment and retries automatically.

  TRIGGERS: call API, use an API, HTTP request, make a request, external data, no API key, paid API, "find info about", "look up", travel, search, scrape, generate image, generate video, social data, send email, validate email, phone call, research, use llm
---

# tempo

Tempo is an agent-first, curl-compatible CLI for discovering services and calling HTTP endpoints with automatic payment handling. Use `tempo wallet` for readiness and service discovery, then use `tempo request` to execute requests against discovered endpoints.

## Setup Contract (For "Set up <url>")

Use this flow when user intent is setup/bootstrap.

### URL Trust Checks

- Accept only `https://` URLs by default.
- Prefer trusted hosts like `tempo.xyz` and `cli.tempo.xyz`.
- If host is unknown or URL does not end with `SKILL.md`, ask for explicit user confirmation before continuing.

### Setup State Machine

```bash
# 0) Resolve a stable user home/bin path (some agent shells have empty HOME)
USER_HOME="${HOME:-$(eval echo "~$(id -un)")}"; USER_BIN="$USER_HOME/.local/bin"

# 1) Ensure CLI exists (agent-safe: install user-local, no sudo)
command -v tempo >/dev/null 2>&1 || (mkdir -p "$USER_BIN" && curl -fsSL https://tempo.xyz/install -o /tmp/tempo_install.sh && TEMPO_BIN_DIR="$USER_BIN" bash /tmp/tempo_install.sh)

# 1b) Ensure user-local bin is on PATH when fallback install is used
export PATH="$USER_BIN:$PATH"

# 2) Validate install
tempo wallet --help

# 3) Check readiness
tempo wallet -t whoami

# 4) Login only if needed (interactive)
tempo wallet login

# 5) Re-check readiness
tempo wallet -t whoami
```

`tempo wallet login` requires user browser/passkey action and opens the auth URL in text mode. Prompt user, wait for confirmation, then continue. Do not loop login attempts without user confirmation.

When run by agents, execute `tempo wallet login` with a long command timeout (at least 16 minutes) so the process can wait for user approval instead of being killed by the runner.

Do not use `sudo` in non-interactive agent shells. Use user-local install via `TEMPO_BIN_DIR="$USER_BIN"`.

### Done Criteria

- `tempo` command executes.
- `tempo wallet -t whoami` returns `ready=true`.

### Setup Completion Output

After setup, provide:

- Installation location and version (`command -v tempo` and `tempo --version`).
- Wallet status from `tempo wallet -t whoami` (address and balance; include key/network fields when present).
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
- If response is a usage/auth readiness error, run required wallet command (usually `tempo wallet login`) and retry once.
- If response indicates payment/funding limit issues, report clearly and stop.

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

## Failure Handling

| Symptom | Action |
|---|---|
| `tempo: command not found` | Run `curl -fsSL https://tempo.xyz/install \| bash`, then retry the original command. |
| Install fails due to permissions/path | Resolve `USER_HOME="${HOME:-$(eval echo "~$(id -un)")}"; USER_BIN="$USER_HOME/.local/bin"`, then `mkdir -p "$USER_BIN" && curl -fsSL https://tempo.xyz/install -o /tmp/tempo_install.sh && TEMPO_BIN_DIR="$USER_BIN" bash /tmp/tempo_install.sh`, then `export PATH="$USER_BIN:$PATH"` and retry. |
| `ready=false` or `No wallet configured` | Run `tempo wallet login`, wait for user completion, then rerun `tempo wallet -t whoami`. |
| Service not found for query | Broaden search terms with `tempo wallet -t services --search <broader_query>`, then inspect candidate details. |
| Endpoint returns usage/path error | Re-open service details with `tempo wallet -t services <SERVICE_ID>` and use discovered method/path exactly. |
| Insufficient funds or spending limit exceeded | Report clearly and stop; ask user to fund or adjust limits before retrying. |
| Timeout/network error | Retry request and optionally increase timeout with `-m <seconds>`. |

## Minimal Command Reference

- `tempo wallet -t whoami` checks wallet readiness and address.
- `tempo wallet -t services --search <query>` finds providers.
- `tempo wallet -t services <SERVICE_ID>` shows service URL, methods, paths, pricing.
- `tempo request -t --dry-run ...` previews cost without paying.
- `tempo request -t ...` executes request and handles payment automatically.
