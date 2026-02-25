# Orphaned Payment Channels: Access Key Expiry & Rotation

## Summary

Payment channels opened via the MPP session protocol can become **orphaned** — unable to be closed cooperatively — when the access key that authorized them expires or is rotated. This leaves deposits locked in the escrow contract with no client-side mechanism to reclaim them.

## The Problem

### How channels work today

1. A **payment channel** is opened on-chain via the escrow contract
2. The channel is bound to an **authorized signer** (the access key) at open time
3. To close a channel, a **voucher** must be signed by that same authorized signer
4. The server submits the signed voucher to the escrow contract's `close()` function

### What goes wrong

Access keys have a **finite lifetime** (currently ~30 days). When a key expires or is rotated:

- The old key's private key is no longer available
- Open channels authorized by that key **cannot produce valid voucher signatures**
- The escrow contract's `close(channelId, cumulativeAmount, signature)` function **rejects** vouchers signed by any other key
- Deposits remain locked in the contract indefinitely

This is not a theoretical concern — during development we observed **4 orphaned channels** each holding 1 USDC, all opened by the same wallet with different access keys over time.

### Root causes

| Cause | Description |
|-------|-------------|
| **Key expiry** | Access keys expire after ~30 days. Channels opened near the end of a key's lifetime may outlive the key. |
| **Key rotation** | `presto login` provisions a new access key without closing channels authorized by the old one. |
| **Local state loss** | If the local session store is lost (e.g., reinstall, different machine), channels can't be recovered without the original key. |

## Impact

- **Locked funds**: Each orphaned channel holds a deposit (typically 1 USDC) that cannot be reclaimed by the payer
- **Channel accumulation**: Repeated key rotations compound the problem, creating multiple orphaned channels per wallet
- **No visibility**: Until `presto session recover` (on-chain scan), users had no way to even discover orphaned channels

## Current Mitigations

### In presto (client-side)

- **On-chain scan before opening**: The session flow (`handle_session_request`) already scans for existing channels on-chain before opening a new one — but only matches channels with the **current** access key
- **`presto session recover`**: Scans on-chain `ChannelOpened` events to discover all open channels for a wallet, regardless of which key opened them

### Implemented fix

Presto now uses the escrow contract's **payer-initiated close** path (`requestClose` → `withdraw`) which does not require the authorized signer's key:

1. `requestClose(channelId)` — called by the payer wallet, starts a 15-minute grace period
2. `withdraw(channelId)` — called by the payer wallet after the grace period, refunds `deposit - settled`

This works for all channels regardless of which access key authorized them. The `close_channel_by_id`, `close_discovered_channel`, and `close_on_chain` functions no longer check for signer matches.

### What's still missing

- **No pre-rotation cleanup**: Key rotation does not attempt to close existing channels first

## Potential Solutions

### 1. Payer-initiated close on the escrow contract (protocol change)

The escrow contract's `getChannel()` already returns a `closeRequestedAt` field, suggesting a timelock dispute pattern may be supported or planned:

```
requestClose(channelId) → starts timelock
  ↓ (dispute window)
finalize(channelId)     → settles at last known amount, returns remaining deposit
```

This would allow the **payer wallet** (not the access key) to unilaterally close a channel after a timeout, regardless of key state. This is the most robust fix.

### 2. Close channels before key rotation (client change)

Before provisioning a new access key, presto could:
1. Scan for all open channels authorized by the current key
2. Send cooperative close requests to each server
3. Only then rotate the key

This doesn't help with expiry but prevents rotation-induced orphaning.

### 3. Retain old keys for signing close vouchers (client change)

Instead of discarding old access keys on rotation, keep them in a "retired keys" store solely for signing close vouchers on existing channels. This adds complexity but preserves the ability to cooperatively close.

### 4. Wallet-level channel authorization (protocol change)

Authorize channels at the **wallet level** rather than the access key level, allowing any valid access key for that wallet to sign close vouchers. This would require changes to the escrow contract's signature verification.

## Discovery & Recovery

```bash
# Close all sessions including orphaned on-chain channels
presto session close --all

# With verbose output
presto -v session close --all

# JSON output
presto session close --all --output-format json

# Specific network only
presto --network tempo session close --all

# Close a specific channel by ID
presto session close 0x<channel_id>
```

## Related

- Escrow contract (mainnet): `0x0901aED692C755b870F9605E56BAA66c35BEfF69`
- Escrow contract (testnet): `0x542831e3E4Ace07559b7C8787395f4Fb99F70787`
- `ChannelOpened` event topic: `0xcd6e60364f8ee4c2b0d62afc07a1fb04fd267ce94693f93f8f85daaa099b5c94`
- MPP spec: https://mpp.sh
