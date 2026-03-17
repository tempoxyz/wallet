---
name: tempo
description: |
  Install and use Tempo to discover and call paid API services. Install user-local, log in, verify. Then: search services, inspect, call with `tempo request`. Never guess URLs.

  TRIGGERS: set up tempo, install tempo, use tempo, "use tempo to", paid API, HTTP request, service discovery
---

# Setup

```bash
# 1. Install (user-local, no sudo)
curl -fsSL https://tempo.xyz/install | TEMPO_BIN_DIR="$HOME/.local/bin" bash

# 2. Log in (opens browser for passkey — wait for user to complete)
"$HOME/.local/bin/tempo" wallet login

# 3. Verify — done when ready=true
"$HOME/.local/bin/tempo" wallet whoami
```

Login requires user interaction (browser/passkey). Set a long command timeout (16+ minutes) and wait.

# Use

```bash
tempo wallet services --search <query>
tempo wallet services <SERVICE_ID>
tempo request -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>
```

Use `--dry-run` before expensive requests to preview cost:

```bash
tempo request --dry-run -X POST --json '{"input":"..."}' <SERVICE_URL>/<ENDPOINT_PATH>
```

# Rules

- Always discover endpoints with `tempo wallet services` first — never guess URLs.
- `tempo request` is curl-compatible (method flags, headers, data, timeouts).
- If `tempo` is not on PATH, use `"$HOME/.local/bin/tempo"`.
- If `tempo: command not found`, rerun the install command.
- If `ready=false`, run `tempo wallet login`, wait for user, then recheck with `tempo wallet whoami`.
- If HTTP 422, inspect `tempo wallet services <SERVICE_ID>` for exact field names.
- If balance is 0, run `tempo wallet fund`.
