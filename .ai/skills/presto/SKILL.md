---
name: presto
description: A wget-like CLI tool for making HTTP requests with automatic payment support
---

# presto

A wget-like CLI tool for making HTTP requests with automatic support for payments.

## Supported Payment Protocols

- **Web Payment Auth** - IETF standard for HTTP authentication-based payments

## Overview

```bash
# Log in (connect your Tempo wallet)
 tempo-walletlogin

# Make a payment-enabled HTTP request
 tempo-walletquery https://api.example.com/premium-data

# Preview payment without executing
 tempo-walletquery --dry-run https://api.example.com/data

# Require confirmation before payment
 tempo-walletquery --confirm https://api.example.com/data
```

## Common Commands

| Command | Description |
|---------|-------------|
| ` tempo-walletlogin` | Log in and connect your Tempo wallet |
| ` tempo-walletlogout` | Log out and disconnect your wallet |
| ` tempo-walletwhoami` | Show wallet status, balances, and access keys |
| ` tempo-walletquery <URL>` (alias ` tempo-walletq <URL>`) | Make an HTTP request (handles 402 payments automatically) |
| ` tempo-wallet--help` | Show all available options |
| ` tempo-walletconfig` | View current configuration |
| ` tempo-walletbalance` | Check wallet balance |

## Key Options

| Option | Description |
|--------|-------------|
| `--dry-run` | Show what would be paid without executing |
| `--confirm` | Require confirmation before paying |
| `--max-amount <AMOUNT>` | Set maximum payment amount (atomic units) |
| `--network <NETWORKS>` | Filter to specific networks (e.g., "tempo","tempo-moderato") |
| `-v, --verbose` | Verbose output with headers |
| `-o, --output <FILE>` | Write output to file |

## Example Usage

```bash
# Basic request with verbose output
 tempo-walletquery -v https://api.example.com/data

# POST request with JSON data
 tempo-walletquery -X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Filter to specific network
 tempo-walletquery --network base-sepolia https://api.example.com/data

# Set maximum payment amount
 tempo-walletquery --max-amount 10000 https://api.example.com/data
```
