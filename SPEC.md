## Error Messages

All error output goes to stderr.

Output concise, informative error messages that include fixes when available.

```
Error: <message with relevant context>

Fix: <one-line actionable instruction>
```

Some errors are fully controlled by presto and use a fixed message. Others pass through a raw error from an external source (RPC node, HTTP server, payment protocol, OS). In those cases, include the raw error in the message so users and support can diagnose the issue.

In the examples below, `{reason}` denotes a passthrough value from the external source.

## Error Scenarios

### SpendingLimitExceeded (exit 6)

Token is displayed as its ticker (e.g., `pathUSD`) when known, or as the contract address (e.g., `0x20c0...`) when unknown.

```
Error: Spending limit exceeded: limit is 0.50 pathUSD, need 1.00 pathUSD

Fix: Run 'presto login' to generate a fresh authorization key.
```

### InsufficientBalance (exit 6)

Token is displayed as its ticker when known, or as the contract address when unknown.

```
Error: Insufficient pathUSD balance: have 0.50, need 1.00

Fix: Deposit funds into your wallet.
```

### InsufficientBalance — no swap source (exit 6)

```
Error: Insufficient pathUSD balance: have 0.00, need 1.00

Fix: Run 'presto balance' to check your balance.
```

### PaymentRejected (exit 5)

The reason is passed through from the server.

When reason contains "insufficient":
```
Error: Payment rejected by server: {reason}

Fix: The price may have changed. Try the request again.
```

Other reasons:
```
Error: Payment rejected by server: {reason}

Fix: Try the request again.
```

### AmountExceedsMax (exit 6)

```
Error: Required amount (1000000) exceeds maximum allowed (500000)

Fix: Increase with --max-amount or remove the limit.
```

### ConfigMissing (exit 3)

```
Error: Configuration missing: {reason}

Fix: Run 'presto login' to set up your wallet.
```

### NoConfigDir (exit 3)

```
Error: Failed to determine config directory

Fix: Set the HOME environment variable.
```

### InvalidConfig (exit 3)

The detail is passed through (e.g., TOML parse error, invalid value).

```
Error: Invalid configuration: {reason}

Fix: Run 'presto config' to view your current configuration.
```

### InvalidKey (exit 8)

```
Error: Invalid private key: {reason}

Fix: EVM private keys should be 64 hex characters (with optional 0x prefix).
```

### Signing / SigningSimple (exit 8)

The signing error is passed through from the signing backend.

```
Error: Signing error: {reason}

Fix: Check your wallet configuration with 'presto config'.
```

### UnknownNetwork (exit 4)

```
Error: Unknown network: {name}

Fix: Run 'presto networks list' to see available networks.
```

### BalanceQuery / SpendingLimitQuery (exit 1)

The RPC error is passed through.

```
Error: Balance query failed: {reason}

Fix: Check your network connection and RPC endpoint.
```

### Http with status codes (exit 4)

The status code and reason phrase are passed through from the server response.

**402:**
```
Error: HTTP error: 402 Payment Required

Fix: Ensure you have a wallet configured with 'presto login'.
```

**401/403:**
```
Error: HTTP error: {status} {reason}

Fix: Check your credentials.
```

**404:**
```
Error: HTTP error: 404 Not Found

Fix: Check the URL.
```

**5xx:**
```
Error: HTTP error: {status} {reason}

Fix: Server error. Try again later.
```

### Generic string-matched errors

| Pattern | Fix |
|---------|-----|
| config not found | `Run 'presto login' to set up your wallet.` |
| permission denied | `Check file permissions or run with appropriate privileges.` |
| connection refused | `Check your internet connection and try again.` |
| timeout | `The request timed out. Try again or use --max-time.` |
