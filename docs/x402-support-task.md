# Task: x402 Protocol Support in tempo-request

## Goal

Enable `tempo-request` to access x402-gated services while keeping **all user interactions on the Tempo chain**. When an x402 service demands payment on another EVM chain (e.g., Base USDC), `tempo-request` bridges funds from Tempo via Relay, then signs a standard x402 EIP-3009 payload for the destination chain — fully compatible with existing x402 facilitators, no server-side changes needed.

## Background

### How x402 Works

1. Client sends `GET /resource`
2. Server returns `402` with `PAYMENT-REQUIRED` header (base64 JSON containing `accepts[]`)
3. Client picks a payment option, signs an off-chain authorization (EIP-3009 `transferWithAuthorization` for USDC)
4. Client retries with `PAYMENT-SIGNATURE` header (base64 JSON with the signed payload)
5. Server/facilitator verifies the signature and on-chain balance, serves content, then settles (calls `USDC.transferWithAuthorization()` on-chain)

The client never submits an on-chain transaction — the facilitator does that and pays gas. The facilitator is chosen by the **server**, not the client.

### How MPP Works (Current)

1. Client sends request
2. Server returns `402` with `WWW-Authenticate: Payment ...` header
3. Client parses MPP challenge, signs a Tempo transaction
4. Client retries with `Authorization: Payment ...` header
5. Server verifies and settles on Tempo

### Key Difference

- **MPP**: `WWW-Authenticate` + `Authorization` headers, Tempo-native
- **x402**: `PAYMENT-REQUIRED` + `PAYMENT-SIGNATURE` headers, multi-chain (Base, Solana, Polygon, etc.)

### Cross-Chain Approach

Since the same secp256k1 private key produces the same address on all EVM chains, we can:

1. Bridge USDC from Tempo → destination chain (to user's own address) via Relay
2. Sign the x402 EIP-3009 authorization with the same key
3. Submit to the server — the facilitator sees a normal funded address

No changes needed on the server or facilitator side. Steps 1 and 2 can run in parallel (signing doesn't require on-chain state), but the bridge must complete before the server verifies `balanceOf()`.

## Scope

### In Scope

- Detect x402 `PAYMENT-REQUIRED` headers (distinct from MPP `WWW-Authenticate`)
- Parse x402 challenge format (`accepts[]` array)
- For same-chain Tempo x402: sign directly (if a server ever supports Tempo)
- For cross-chain EVM x402: bridge via Relay + sign EIP-3009
- secp256k1 keys only (same address on all EVM chains)
- Display bridge fees + payment amount before executing
- Dry-run support (show quote without paying)
- Analytics tracking for x402 payments

### Out of Scope (Future)

- Solana (SVM) x402 payments (different key type)
- p256/webauthn key support for cross-chain (no EVM address equivalence)
- Building a Tempo x402 facilitator (server-side work)
- Session/streaming x402 payments (x402 currently only supports one-shot)

## Implementation Plan

### Phase 1: x402 Challenge Detection & Parsing

**Goal**: Detect x402 402 responses and parse challenges, without executing payment yet.

#### 1.1 Add x402 types (`crates/tempo-request/src/payment/x402/types.rs`)

Data structures for x402 protocol messages:

```rust
/// Decoded PAYMENT-REQUIRED header.
pub(crate) struct X402PaymentRequired {
    pub x402_version: u32,
    pub error: Option<String>,
    pub resource: X402Resource,
    pub accepts: Vec<X402PaymentOption>,
}

/// A single payment option from accepts[].
pub(crate) struct X402PaymentOption {
    pub scheme: String,          // "exact"
    pub network: String,         // CAIP-2: "eip155:8453"
    pub amount: String,          // atomic units: "10000"
    pub asset: String,           // token contract address
    pub pay_to: String,          // merchant address
    pub max_timeout_seconds: u64,
    pub extra: serde_json::Value, // scheme-specific (name, version, etc.)
}

/// Resource metadata.
pub(crate) struct X402Resource {
    pub url: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// The PAYMENT-SIGNATURE payload sent back to the server.
pub(crate) struct X402PaymentPayload {
    pub x402_version: u32,
    pub resource: X402Resource,
    pub accepted: X402PaymentOption,  // echoed back from accepts[]
    pub payload: serde_json::Value,   // scheme-specific signed data
}
```

#### 1.2 Parse x402 challenges (`crates/tempo-request/src/payment/x402/challenge.rs`)

- Decode `PAYMENT-REQUIRED` header (base64 → JSON → `X402PaymentRequired`)
- Select best payment option from `accepts[]`:
  1. Prefer Tempo-native (`eip155:4217`) if available
  2. Otherwise pick the first supported EVM chain
- Extract chain ID from CAIP-2 network string (`eip155:8453` → `8453`)
- Validate: scheme must be `exact`, network must be EVM (`eip155:*`)

#### 1.3 Update 402 detection (`crates/tempo-request/src/query/mod.rs` + `challenge.rs`)

Current flow checks for `WWW-Authenticate` header. Update to:

```
if response.status == 402 {
    if has_header("payment-required") {
        → x402 flow
    } else if has_header("www-authenticate") {
        → MPP flow (existing)
    } else {
        → error: unknown 402 protocol
    }
}
```

Priority: check `PAYMENT-REQUIRED` first (x402), then `WWW-Authenticate` (MPP).

#### 1.4 Update router (`crates/tempo-request/src/payment/router.rs`)

Add `dispatch_x402_payment()` alongside existing `dispatch_payment()`. Initially returns an error ("x402 cross-chain payments not yet supported") for non-Tempo chains.

---

### Phase 2: EIP-3009 Signing

**Goal**: Sign `transferWithAuthorization` payloads using the user's secp256k1 key.

#### 2.1 EIP-3009 signer (`crates/tempo-request/src/payment/x402/eip3009.rs`)

Implement EIP-712 typed data signing for `TransferWithAuthorization`:

```
Domain:
  name: <from extra.name, e.g. "USDC">
  version: <from extra.version, e.g. "2">
  chainId: <from CAIP-2 network>
  verifyingContract: <asset address>

Message (TransferWithAuthorization):
  from: <user's address>
  to: <payTo>
  value: <amount>
  validAfter: <now>
  validBefore: <now + maxTimeoutSeconds>
  nonce: <random 32 bytes>
```

Sign with the user's secp256k1 key via alloy's `SignerSync` trait (already available through the keystore's signer).

Return the structured payload:
```json
{
  "signature": "0x...",
  "authorization": { "from", "to", "value", "validAfter", "validBefore", "nonce" }
}
```

#### 2.2 Build PAYMENT-SIGNATURE header (`crates/tempo-request/src/payment/x402/payload.rs`)

- Construct `X402PaymentPayload` (echo `resource` + `accepted` + signed `payload`)
- Serialize to JSON, base64-encode
- Return as header value for `PAYMENT-SIGNATURE`

#### 2.3 Submit and handle response

- Retry the original request with `PAYMENT-SIGNATURE` header
- Parse `PAYMENT-RESPONSE` header from successful response (settlement receipt)
- Display receipt: tx hash, network, payer

---

### Phase 3: Relay Bridge Integration

**Goal**: Bridge USDC from Tempo to the destination chain before signing.

#### 3.1 Relay client (`crates/tempo-request/src/payment/x402/relay.rs`)

Minimal Relay REST API client using existing `reqwest`:

```rust
/// Get a quote for bridging from Tempo to destination chain.
pub(crate) async fn get_quote(
    user: Address,
    destination_chain_id: u64,
    destination_asset: Address,
    amount: &str,
) -> Result<RelayQuote, TempoError>

/// Execute the quote (returns steps with tx data to sign+submit).
/// Poll status until bridge completes.
pub(crate) async fn execute_and_wait(
    quote: &RelayQuote,
    signer: &Signer,
    rpc_url: &Url,
) -> Result<RelayResult, TempoError>
```

API calls:
- `POST https://api.relay.link/quote` — get bridge quote
  - `user`: user's address
  - `originChainId`: Tempo chain ID (4217)
  - `destinationChainId`: from x402 challenge
  - `originCurrency`: USDC on Tempo
  - `destinationCurrency`: asset from x402 challenge
  - `amount`: from x402 challenge
  - `tradeType`: `"EXACT_OUTPUT"` (ensure destination receives exact amount needed)
  - `recipient`: user's own address (same key = same address)
- `GET https://api.relay.link/intents/status/v3?requestId=...` — poll until `status: "success"`

#### 3.2 Cross-chain payment flow (`crates/tempo-request/src/payment/x402/flow.rs`)

Orchestrate the full cross-chain flow:

```
┌─────────────────────────────────────────────────┐
│ 1. Parse x402 challenge                         │
│ 2. Check: is destination chain == Tempo?        │
│    ├─ YES → sign EIP-3009 directly, skip bridge │
│    └─ NO → continue to step 3                   │
│ 3. Display: "Bridge 0.01 USDC Tempo→Base        │
│             Bridge fee: ~0.002 USDC              │
│             Total: ~0.012 USDC from Tempo"       │
│ 4. In parallel:                                  │
│    ├─ a. Bridge via Relay (Tempo → dest chain)   │
│    └─ b. Sign EIP-3009 for dest chain            │
│ 5. Wait for bridge completion                    │
│ 6. Submit PAYMENT-SIGNATURE to server            │
│ 7. Display receipt                               │
└─────────────────────────────────────────────────┘
```

#### 3.3 Dry-run support

When `--dry-run` is set:
- Get Relay quote (shows fees)
- Display what would be signed
- Skip bridge execution and payment submission

---

### Phase 4: Error Handling & UX

#### 4.1 Error types (`crates/tempo-common/src/error.rs`)

Add x402-specific error variants:

```rust
pub enum PaymentError {
    // ... existing variants ...

    // x402 errors
    #[error("x402: no supported payment option in accepts[]")]
    X402NoSupportedOption,
    #[error("x402: unsupported scheme '{0}' (only 'exact' is supported)")]
    X402UnsupportedScheme(String),
    #[error("x402: unsupported network '{0}' (only EVM chains supported)")]
    X402UnsupportedNetwork(String),
    #[error("x402: bridge failed: {0}")]
    X402BridgeFailed(String),
    #[error("x402: bridge timed out after {0}s")]
    X402BridgeTimeout(u64),
    #[error("x402: payment rejected by server: {reason}")]
    X402PaymentRejected { reason: String, status_code: u16 },
    #[error("x402: cross-chain requires secp256k1 key (passkey wallets not supported)")]
    X402PasskeyNotSupported,
}
```

#### 4.2 Analytics (`crates/tempo-request/src/query/analytics.rs`)

Track x402-specific events:
- `x402_payment_started` — protocol, scheme, source chain, dest chain, amount
- `x402_bridge_started` / `x402_bridge_completed` — bridge timing
- `x402_payment_success` / `x402_payment_failure`

#### 4.3 Display

- Show bridge progress: "Bridging 0.01 USDC from Tempo to Base..."
- Show bridge completion: "Bridge complete (2.3s)"
- Show payment receipt: tx hash, network, amount
- Verbose mode (`-v`): show full Relay quote, EIP-712 domain, signed payload

---

## Architecture

### New Files

```
crates/tempo-request/src/payment/x402/
├── mod.rs          # Module declarations
├── types.rs        # X402PaymentRequired, X402PaymentOption, X402PaymentPayload
├── challenge.rs    # Parse PAYMENT-REQUIRED header, select payment option
├── eip3009.rs      # EIP-712 signing for TransferWithAuthorization
├── payload.rs      # Build PAYMENT-SIGNATURE header
├── relay.rs        # Relay REST API client (quote, execute, status)
└── flow.rs         # Orchestrate: bridge → sign → submit
```

### Modified Files

```
crates/tempo-request/src/query/mod.rs        # Add x402 402 detection branch
crates/tempo-request/src/query/challenge.rs  # Add x402 header check
crates/tempo-request/src/payment/mod.rs      # Add x402 module declaration
crates/tempo-request/src/payment/router.rs   # Add x402 dispatch path
crates/tempo-common/src/error.rs             # Add x402 error variants
crates/tempo-request/src/query/analytics.rs  # Add x402 analytics events
```

### Dependencies

No new crate dependencies required:
- `alloy` — already available, has EIP-712 signing support
- `reqwest` — already available, for Relay API calls
- `serde` / `serde_json` — already available, for x402 JSON parsing
- `base64` — already available, for header encoding/decoding

---

## Constraints

1. **secp256k1 only** — Cross-chain x402 requires the same address on all EVM chains. p256/webauthn keys don't produce valid EVM addresses on other chains. Fail with a clear error for non-secp256k1 keys.
2. **EVM only** — Solana x402 uses a completely different key type and payload format. Out of scope.
3. **`exact` scheme only** — This is the only scheme in production today. `upto` can be added later.
4. **Bridge before verify** — The Relay bridge must complete before we submit to the x402 server, because verification checks `balanceOf()` on the destination chain.
5. **Timeout budget** — x402 challenges have `maxTimeoutSeconds` (typically 60s). The bridge (~3s) + verification + settlement must all fit within this window.

## Success Criteria

- `tempo-request https://some-x402-api.com/endpoint` works for x402-gated services
- User only needs funds on Tempo, regardless of destination chain
- Bridge fees + payment amount shown before execution
- `--dry-run` shows the bridge quote without executing
- Existing MPP flows remain completely unaffected
- `make check` passes with zero issues
