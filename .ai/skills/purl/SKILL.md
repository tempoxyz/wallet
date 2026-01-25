---
name: purl
description: A curl-like CLI tool for making HTTP requests with automatic payment support
---

# purl - p(ay)URL

A curl-like CLI tool for making HTTP requests with automatic support for payments.

## Supported Payment Protocols

- **Web Payment Auth** - IETF standard for HTTP authentication-based payments

## Overview

```bash
# Initialize configuration (first time setup)
purl init

# Make a payment-enabled HTTP request
purl https://api.example.com/premium-data

# Preview payment without executing
purl --dry-run https://api.example.com/data

# Require confirmation before payment
purl --confirm https://api.example.com/data
```

## Common Commands

| Command | Description |
|---------|-------------|
| `purl init` | Initialize or reconfigure your purl setup |
| `purl <URL>` | Make an HTTP request (handles 402 payments automatically) |
| `purl --help` | Show all available options |
| `purl config` | View current configuration |
| `purl balance` | Check wallet balance |
| `purl method list` | List available payment methods/keystores |

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
purl -v https://api.example.com/data

# POST request with JSON data
purl -X POST --json '{"key": "value"}' https://api.example.com/endpoint

# Filter to specific network
purl --network base-sepolia https://api.example.com/data

# Set maximum payment amount
purl --max-amount 10000 https://api.example.com/data
```
