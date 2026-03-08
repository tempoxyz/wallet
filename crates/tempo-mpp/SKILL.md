---
name: tempo mpp
description: "MPP session and service management — manage payment sessions and browse the MPP service directory. Use `tempo mpp -t services` to discover available services, `tempo mpp sessions list` to view active payment sessions."
---

# tempo mpp

MPP session and service management extension for the Tempo CLI. Browse available services, manage payment sessions and channels. For making HTTP requests with automatic payment, use `tempo request`.

**Use tempo mpp when you need to:**
- Discover available MPP services and their endpoints (`services`)
- List, inspect, or close payment sessions (`sessions`)
- Manage on-chain payment channels

## Available Services

Use `tempo mpp services` to discover available services, their endpoints, and pricing:

```bash
# List all available services
tempo mpp -t services

# Filter by category (ai, search, compute, blockchain, data, media, social, storage, web)
tempo mpp -t services --category ai

# Search by name, description, or tags
tempo mpp -t services --search <QUERY>

# Show full details for a service (endpoints, pricing, docs)
tempo mpp -t services info <SERVICE_ID>
```

Each service is accessed via its MPP service URL (shown in the `Service URL` column of `tempo mpp services`). When you don't know which service or endpoint to use, run `tempo mpp services info <id>` to see every endpoint with its HTTP method, path, pricing, and documentation links.

## Sessions

Sessions open a payment channel on-chain once, then use off-chain vouchers for subsequent requests — no gas per request. Sessions are created automatically by `tempo request` when a service requests one.

### Session States

| State | Meaning |
|-------|---------|
| active | Channel open and usable |
| closing | Close requested; grace period in progress |
| finalizable | Grace period elapsed; ready to withdraw |
| orphaned | On-chain channel with no local record |

### Managing Sessions

```bash
# List active sessions
tempo mpp -t sessions list

# List all sessions (including closing/finalizable)
tempo mpp -t sessions list --state all

# Show details for a session by URL or channel ID
tempo mpp -t sessions info <URL|channel_id>

# Close a specific session
tempo mpp -t sessions close <URL>

# Close all sessions
tempo mpp -t sessions close --all

# Finalize channels ready to withdraw
tempo mpp -t sessions close --finalize

# Close orphaned on-chain channels (no local record)
tempo mpp -t sessions close --orphaned

# Re-sync a session from on-chain state (after crash/manual edit)
tempo mpp -t sessions sync --origin <URL>

# Remove stale local records (already settled on-chain)
tempo mpp -t sessions sync
```

## Commands

| Command | Description |
|---------|-------------|
| `tempo mpp services` | List available MPP services |
| `tempo mpp services --category ai` | Filter services by category |
| `tempo mpp services --search <QUERY>` | Search services by name, description, or tags |
| `tempo mpp services info <ID>` | Show detailed info for a service (endpoints, pricing, docs) |
| `tempo mpp sessions list` | List payment sessions |
| `tempo mpp sessions info <URL\|channel_id>` | Show details for a session or channel |
| `tempo mpp sessions close [--all\|--orphaned\|--finalize\|<URL>]` | Close sessions or channels |
| `tempo mpp sessions sync` | Remove stale local sessions (settled on-chain) |
| `tempo mpp sessions sync --origin <URL>` | Re-sync a session's close state from chain |

## Global Options

| Option | Description |
|--------|-------------|
| `-v` | Verbose output (`-vv` debug, `-vvv` trace) |
| `-s, --silent` | Suppress non-essential stderr output |
| `-t, --toon-output` | TOON output — compact, token-efficient (recommended for agents) |
| `-j, --json-output` | JSON output |
