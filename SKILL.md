---
name: tempo
description: |
 Install and set up Tempo — an agent-first CLI for discovering services and calling HTTP APIs with built-in payments. One command to install, one to log in, then discover and use any service.

 TRIGGERS: set up tempo, install tempo, use tempo, call API, paid API, HTTP request, service discovery, "use tempo to"
---

# Tempo

Tempo gives your agent pay-per-call access to premium APIs with automatic payment handling. No API keys, no subscriptions.

## Setup

Run these steps in order. Skip any step that's already done.

```bash
# 1. Install (user-local, no sudo)
curl -fsSL https://tempo.xyz/install | TEMPO_BIN_DIR="$HOME/.local/bin" bash

# 2. Log in (opens browser for passkey — wait for user to complete)
"$HOME/.local/bin/tempo" wallet login

# 3. Verify
"$HOME/.local/bin/tempo" wallet whoami
```

Setup is done when `whoami` returns `ready=true`.

**Notes:**
- `tempo wallet login` requires user interaction (browser/passkey). Set a long command timeout (16+ minutes) and wait for confirmation.
- If `$HOME` doesn't expand, use the absolute path directly (e.g., `/Users/alice/.local/bin/tempo`).
- If balance is 0, run `tempo wallet fund` to add funds.

## After Setup

```bash
# Find services
tempo wallet services --search <query>

# Inspect a service (shows URL, methods, paths, pricing)
tempo wallet services <SERVICE_ID>

# Make a request (handles payment automatically)
tempo request -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>

# Preview cost before paying
tempo request --dry-run -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>

# Check balance
tempo wallet whoami
```

**Rules:**
- Always discover endpoints with `tempo wallet services` before making requests — never guess URLs.
- Use `--dry-run` before expensive requests.
- `tempo request` is curl-compatible (method flags, headers, data, redirects, timeouts).
- If you get HTTP 422, check `tempo wallet services <SERVICE_ID>` for exact field names.

## Common Issues

| Issue | Fix |
|---|---|
| `tempo: command not found` | Run the install command from step 1, then use full path `"$HOME/.local/bin/tempo"`. |
| `ready=false` | Run `tempo wallet login`, wait for user, then `tempo wallet whoami`. |
| Balance is 0 | Run `tempo wallet fund`. |
| "legacy V1 keychain" error | Reinstall: `curl -fsSL https://tempo.xyz/install \| bash`, then `tempo wallet logout --yes && tempo wallet login`. |
| Service not found | Broaden search: `tempo wallet services --search <broader_query>`. |

## Support

- **Dashboard**: https://wallet.tempo.xyz
- **Docs**: https://docs.tempo.xyz
