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
presto login

# Make a payment-enabled HTTP request
presto query https://api.example.com/premium-data

# Preview payment without executing
presto query --dry-run https://api.example.com/data

# Require confirmation before payment
presto query --confirm https://api.example.com/data
```

## Common Commands

| Command | Description |
|---------|-------------|
| `presto login` | Log in and connect your Tempo wallet |
| `presto logout` | Log out and disconnect your wallet |
| `presto whoami` | Show wallet status, balances, and access keys |
| `presto query <URL>` (alias `presto q <URL>`) | Make an HTTP request (handles 402 payments automatically) |
| `presto --help` | Show all available options |
| `presto config` | View current configuration |
| `presto balance` | Check wallet balance |

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
presto query -v https://api.example.com/data

# POST request with JSON data
presto query -X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Filter to specific network
presto query --network base-sepolia https://api.example.com/data

# Set maximum payment amount
presto query --max-amount 10000 https://api.example.com/data
```
