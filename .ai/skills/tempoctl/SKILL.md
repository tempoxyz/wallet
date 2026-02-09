---
name: tempoctl
description: A wget-like CLI tool for making HTTP requests with automatic payment support
---

# tempoctl

A wget-like CLI tool for making HTTP requests with automatic support for payments.

## Supported Payment Protocols

- **Web Payment Auth** - IETF standard for HTTP authentication-based payments

## Overview

```bash
# Log in (connect your Tempo wallet)
tempoctl login

# Make a payment-enabled HTTP request
tempoctl query https://api.example.com/premium-data

# Preview payment without executing
tempoctl query --dry-run https://api.example.com/data

# Require confirmation before payment
tempoctl query --confirm https://api.example.com/data
```

## Common Commands

| Command | Description |
|---------|-------------|
| `tempoctl login` | Log in and connect your Tempo wallet |
| `tempoctl logout` | Log out and disconnect your wallet |
| `tempoctl whoami` | Show wallet status, balances, and access keys |
| `tempoctl query <URL>` (alias `tempoctl q <URL>`) | Make an HTTP request (handles 402 payments automatically) |
| `tempoctl --help` | Show all available options |
| `tempoctl config` | View current configuration |
| `tempoctl balance` | Check wallet balance |

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
tempoctl query -v https://api.example.com/data

# POST request with JSON data
tempoctl query -X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Filter to specific network
tempoctl query --network base-sepolia https://api.example.com/data

# Set maximum payment amount
tempoctl query --max-amount 10000 https://api.example.com/data
```
