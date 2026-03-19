# Task: x402 Protocol Support in tempo-request (Prototype)

## Requirements

1. **Access x402 services**: `tempo-request` must handle x402's HTTP 402 flow alongside existing MPP support
2. **All user transactions on Tempo**: Only on-chain txs are on Tempo (Relay approve + deposit). The EIP-3009 authorization is off-chain (no gas, no chain interaction)
3. **No server-side changes**: Works with existing x402 facilitators unmodified
4. **Cross-chain via Relay**: Bridge funds from Tempo to destination chain (Relay confirmed supporting Tempo, chain 4217)

## Approach

Same secp256k1 key = same address on all EVM chains. Flow:

1. **Bridge**: Relay moves USDC from Tempo → destination chain to user's own address
2. **Sign**: Off-chain EIP-3009 `transferWithAuthorization` for destination chain (after bridge completes)
3. **Submit**: `PAYMENT-SIGNATURE` header → server sees a normal funded address

## Prototype Scope

**This is a prototype.** Minimal implementation to prove the e2e flow works.

### In Scope

- Detect `PAYMENT-REQUIRED` header and route to x402 handler
- Parse x402 v2 challenge, pick first `exact` EVM EIP-3009 option from `accepts[]`
- Bridge USDC via Relay (iterate steps: approve + deposit, poll for completion)
- Sign EIP-3009 with user's secp256k1 key (after bridge completes)
- Build `PAYMENT-SIGNATURE` header, retry request, show response

### Cut (Not in Prototype)

- Analytics / telemetry
- Dry-run support
- Fee display / user confirmation before bridging
- Smart option selection from `accepts[]` (just take the first valid option)
- Tempo-native x402 path (no bridge needed — can add later)
- Verbose mode output
- Permit2 / ERC-7710 asset transfer methods
- x402 v1 support

## Constraints

1. **Direct EOA only** — The "same address on all EVM chains" assumption requires `wallet_address == key_address` (i.e., `TempoSigningMode::Direct`). Keychain/passkey/webauthn wallets have `wallet_address != key_address`, so the bridge recipient and EIP-3009 `from` would be different addresses. Fail fast if the signer is not a direct EOA.
2. **Tempo mainnet origin only** — Relay bridge is from chain 4217. Fail fast on Moderato.
3. **EVM destination only** — Only `eip155:*` networks in x402 `accepts[]`. No Solana.
4. **`exact` scheme + `eip3009` method only** — Filter `accepts[]` for `scheme == "exact"` with `extra.assetTransferMethod` absent or `"eip3009"`. Reject `permit2` / `erc7710`.
5. **x402 v2 only** — Use `PAYMENT-REQUIRED` / `PAYMENT-SIGNATURE` / `PAYMENT-RESPONSE` headers. Ignore v1 legacy formats.
6. **Bridge before sign** — Sign EIP-3009 after bridge completes (not in parallel). Avoids signature expiry risk if bridge is slow.
7. **Cannot reuse existing `NetworkId`** — `NetworkId` only knows Tempo and Moderato. Destination chains (Base, Ethereum, etc.) are represented as raw CAIP-2 strings / `u64` chain IDs within the x402 module.
8. **Cannot reuse existing MPP routing** — x402 does not flow through `dispatch_payment()`, `parse_payment_challenge()`, or the existing wallet-login flow. It's a fully separate path.

---

## Implementation Plan

### Step 1: x402 Module Skeleton

| File | What |
|------|------|
| `payment/x402/mod.rs` | Module root. Single public entry: `pub(crate) async fn run()`. Submodule declarations. |
| `payment/x402/types.rs` | Serde structs for x402 v2: `X402PaymentRequired` (with `x402_version`, `error`, `resource`, `accepts`), `X402PaymentOption` (with `scheme`, `network`, `amount`, `asset`, `pay_to`, `max_timeout_seconds`, `extra`), `X402Resource`, `X402PaymentPayload`. Use `#[serde(rename_all = "camelCase")]` to match the JSON field names (e.g., `payTo`, `maxTimeoutSeconds`, `x402Version`). |
| `payment/mod.rs` | Add `pub(crate) mod x402;` |
| `query/mod.rs` | Add `if` branch after `effective_url` computation (line 97) and before `challenge::parse_payment_challenge` (line 99): check `payment-required` header → call `x402::run()` and return. This keeps the existing MPP path completely untouched. |

### Step 2: Challenge Parsing

| File | What |
|------|------|
| `payment/x402/challenge.rs` | Decode `PAYMENT-REQUIRED` header: base64 (standard, not URL-safe) → JSON → `X402PaymentRequired`. Pick first option where `scheme == "exact"` AND `network` starts with `eip155:` AND `extra.assetTransferMethod` is absent or `"eip3009"`. Extract chain ID from CAIP-2 string (`eip155:8453` → `8453`). Require `extra.name` and `extra.version` (needed for EIP-712 domain). Return the selected option + parsed chain ID. |

### Step 3: EIP-3009 Signing

| File | What |
|------|------|
| `payment/x402/eip3009.rs` | Define EIP-712 struct via alloy `sol!` macro for `TransferWithAuthorization { address from, address to, uint256 value, uint256 validAfter, uint256 validBefore, bytes32 nonce }`. Build `Eip712Domain` from challenge: `name` from `extra.name`, `version` from `extra.version`, `chain_id` from CAIP-2, `verifying_contract` from `asset`. Sign with `PrivateKeySigner` (async `sign_typed_data`). Return `(signature_hex, authorization_params)`. |

### Step 4: Payload Construction & Submission

| File | What |
|------|------|
| `payment/x402/payload.rs` | Build `X402PaymentPayload`: echo `resource` + selected `accepted` option exactly as received from server (use `serde_json::Value` to preserve original field structure) + signed `payload` (`signature`, `authorization`). Serialize to JSON, base64-encode (standard) for `PAYMENT-SIGNATURE` header. |

### Step 5: Relay Bridge

| File | What |
|------|------|
| `payment/x402/relay.rs` | **`get_quote()`**: `POST https://api.relay.link/quote/v2` with `{user, originChainId: 4217, destinationChainId, originCurrency: USDC_ON_TEMPO, destinationCurrency, amount, tradeType: "EXACT_OUTPUT", recipient: user}`. Returns steps + fees. **`execute_steps()`**: iterate returned `steps[]` array. For each step, iterate `items[]`. Each item has `data` (a tx to sign+submit) with `{from, to, data, value, chainId, gas, maxFeePerGas, maxPriorityFeePerGas}`. Build an alloy `TransactionRequest` from these fields, sign with `PrivateKeySigner`, submit via an alloy HTTP provider connected to Tempo RPC (get URL from `config.rpc_url(NetworkId::Tempo)`). After the deposit step's tx is confirmed, use the item's `check.endpoint` field (prefix with `https://api.relay.link` if relative) and `check.method` to poll status. Poll until `status == "success"`, fail on terminal states (`failure`, `refunded`). Fail fast on any non-`"transaction"` step kind. |

### Step 6: Wire It Together

| File | What |
|------|------|
| `payment/x402/mod.rs` | `run()` orchestrates: (1) parse challenge, (2) get `PrivateKeySigner` from keystore — use `ctx.keys.signer(NetworkId::Tempo)`, verify `signing_mode` is `Direct` (fail if keychain), extract `.signer` field for the raw `PrivateKeySigner`, (3) get Relay quote, (4) execute Relay steps (approve + deposit txs on Tempo), (5) wait for bridge completion, (6) sign EIP-3009 with same `PrivateKeySigner`, (7) build `PAYMENT-SIGNATURE` header, (8) retry original request via `http.execute(url, &[(PAYMENT-SIGNATURE, value)])`, (9) pass response to `output::handle_response()` for display |

---

## Architecture

### Encapsulation

Two touch points on existing code, everything else inside `x402/`:

```
query/mod.rs (existing, after effective_url computation, before MPP parse)
│  if response.header("payment-required").is_some() {
│      return crate::payment::x402::run(
│          ctx, &prepared.http, &response, &effective_url, &output_opts,
│      ).await;
│  }
│  // ... existing MPP flow unchanged below ...

payment/
├── mod.rs              ← one line added: pub(crate) mod x402;
├── charge.rs           (unchanged)
├── session/            (unchanged)
├── router.rs           (unchanged)
├── types.rs            (unchanged)
└── x402/
    ├── mod.rs           # run() entry point
    ├── types.rs         # x402 v2 serde structs
    ├── challenge.rs     # parse PAYMENT-REQUIRED, select option
    ├── eip3009.rs       # EIP-712 signing (sol! macro + sign_typed_data)
    ├── payload.rs       # build PAYMENT-SIGNATURE header
    └── relay.rs         # Relay API: quote/v2 + step execution + status polling
```

### Key Differences from MPP Path

| Concern | MPP (existing) | x402 (new) |
|---------|---------------|------------|
| 402 header | `WWW-Authenticate` | `PAYMENT-REQUIRED` |
| Payment header | `Authorization` | `PAYMENT-SIGNATURE` |
| Network model | `NetworkId` (Tempo/Moderato only) | Raw CAIP-2 string / `u64` chain ID |
| Wallet requirement | Any key type | Direct EOA only (`signer.address() == wallet_address`) |
| Routing | `dispatch_payment()` → charge/session | `x402::run()` → bridge → sign → submit |
| Chain | Always Tempo | Any EVM chain (bridged from Tempo) |

### Dependencies

No new crate dependencies. Uses existing:
- `alloy` — EIP-712 signing (`sol!`, `Signer::sign_typed_data`, `PrivateKeySigner`, `Provider` for bridge tx submission)
- `reqwest` — Relay API calls
- `serde` / `serde_json` — JSON
- `base64` — header encoding

## Success Criteria

- `tempo-request https://some-x402-api.com/endpoint` completes an x402 payment end-to-end
- User only needs USDC on Tempo
- Existing MPP flows unaffected
- `make check` passes
