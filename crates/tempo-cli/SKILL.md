---
name: tempo
description: "Tempo CLI launcher — dispatches commands to wallet and mpp extensions. Install with: curl -fsSL https://cli.tempo.xyz/install | bash"
---

# tempo

The top-level Tempo CLI launcher. It dispatches commands to extension binaries:

- `tempo wallet ...` → wallet identity and custody
- `tempo mpp ...` → HTTP client with automatic payment

## Install

```bash
curl -fsSL https://cli.tempo.xyz/install | bash
```

## Usage

```bash
# Wallet management
tempo wallet login
tempo wallet whoami
tempo wallet keys list

# HTTP requests with automatic payment
tempo mpp <URL> -X POST --json '...'
tempo mpp services
tempo mpp sessions list

# Browse services
tempo mpp -t services
```

See the tempo-wallet SKILL.md for wallet commands and tempo-mpp SKILL.md for HTTP/payment commands.
