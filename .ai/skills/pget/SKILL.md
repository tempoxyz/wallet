---
name: pget
description: A wget-like CLI tool for making HTTP requests with automatic payment support
---

# pget - p(ay)GET

A wget-like CLI tool for making HTTP requests with automatic support for payments.

## Supported Payment Protocols

- **Web Payment Auth** - IETF standard for HTTP authentication-based payments

## Overview

```bash
# Initialize configuration (first time setup)
pget init

# Make a payment-enabled HTTP request
pget https://api.example.com/premium-data

# Preview payment without executing
pget --dry-run https://api.example.com/data

# Require confirmation before payment
pget --confirm https://api.example.com/data
```

## Common Commands

| Command | Description |
|---------|-------------|
| `pget init` | Initialize or reconfigure your pget setup |
| `pget <URL>` | Make an HTTP request (handles 402 payments automatically) |
| `pget --help` | Show all available options |
| `pget config` | View current configuration |
| `pget balance` | Check wallet balance |
| `pget method list` | List available payment methods/keystores |

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
pget -v https://api.example.com/data

# POST request with JSON data
pget -X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Filter to specific network
pget --network base-sepolia https://api.example.com/data

# Set maximum payment amount
pget --max-amount 10000 https://api.example.com/data
```
