# purl

A Rust library for implementing the [Web Payment Auth protocol](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) (IETF draft).

This library provides everything needed to build applications that can make HTTP requests with automatic payment handling, supporting multiple blockchain networks (EVM, Solana) and payment protocols.

## Features

- **Protocol Implementation**: Core Web Payment Auth types, parsing, and encoding
- **Multiple Providers**: Built-in support for EVM (Ethereum, Base, Polygon, etc.) and Solana chains
- **HTTP Client**: Payment-enabled HTTP client with automatic 402 handling (optional)
- **Keystore Management**: Secure encrypted key storage using Ethereum keystore v3 format (optional)
- **Type-safe**: Strongly-typed protocol definitions and configuration
- **Modular**: Fine-grained feature flags let you include only what you need
- **Async**: Built on tokio for async/await support

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
purl = "0.2"
```

### Making Payment-Enabled HTTP Requests

```rust
use purl::{Client, Config, EvmConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure with your payment method
    let config = Config {
        evm: Some(EvmConfig {
            keystore: Some("/path/to/keystore.json".into()),
            private_key: None,
        }),
        ..Default::default()
    };

    // Create client with payment settings
    let client = Client::with_config(config)
        .max_amount("1000000")  // Max 1 USDC (6 decimals)
        .allowed_networks(&["base"]);

    // Make request - payment is automatic if server returns 402
    let result = client.get("https://api.example.com/premium-data").await?;

    println!("Response: {:?}", result);
    Ok(())
}
```

### Using as a Protocol Library

If you just need the protocol types and parsing (no HTTP client):

```toml
[dependencies]
purl = { version = "0.2", default-features = false, features = ["web-payment"] }
```

```rust
use purl::protocol::{parse_www_authenticate, format_authorization};

// Parse WWW-Authenticate header from 402 response
let challenge = parse_www_authenticate(header_value)?;

// Create payment credential (you provide your own provider)
let credential = my_provider.create_payment(&challenge).await?;

// Format Authorization header for retry request
let auth_header = format_authorization(&credential)?;
```

### Building a Custom Provider

Implement the `PaymentProvider` trait to add support for new blockchains:

```rust
use purl::provider::PaymentProvider;
use purl::protocol::PaymentChallenge;
use async_trait::async_trait;

pub struct MyCustomProvider {
    // Your provider state
}

#[async_trait]
impl PaymentProvider for MyCustomProvider {
    fn supports_network(&self, network: &str) -> bool {
        network == "my-chain"
    }

    fn name(&self) -> &str {
        "MyChain"
    }

    async fn get_balance(
        &self,
        address: &str,
        network: &Network,
        currency: &Currency,
    ) -> Result<NetworkBalance> {
        // Implement balance checking
    }

    async fn create_web_payment(
        &self,
        challenge: &PaymentChallenge,
        config: &Config,
    ) -> Result<PaymentCredential> {
        // Implement payment creation
    }
}
```

## Feature Flags

The library uses fine-grained features for minimal dependencies:

| Feature | Description | Default |
|---------|-------------|---------|
| `web-payment` | Core protocol types (no deps) | ✓ |
| `http-client` | HTTP client using curl | ✓ |
| `client` | High-level `Client` API | ✓ |
| `keystore` | Encrypted keystore management | ✓ |
| `evm` | EVM provider (Ethereum, Base, etc.) | ✓ |
| `solana` | Solana provider | ✓ |
| `full` | All features | - |

### Example Configurations

**Minimal protocol types only:**
```toml
purl = { version = "0.2", default-features = false, features = ["web-payment"] }
```

**EVM only (no Solana):**
```toml
purl = { version = "0.2", default-features = false, features = ["evm", "client"] }
```

**Full featured:**
```toml
purl = { version = "0.2", features = ["full"] }
```

## Module Overview

- **`protocol`**: Web Payment Auth protocol types and parsing
  - `protocol::web`: Core types like `PaymentChallenge`, `PaymentCredential`, `PaymentReceipt`
- **`provider`**: Payment provider abstraction
  - `provider::PaymentProvider`: Trait for blockchain providers
  - `provider::evm::EvmProvider`: EVM implementation (feature: `evm`)
  - `provider::solana::SolanaProvider`: Solana implementation (feature: `solana`)
- **`client`**: High-level client API (feature: `client`)
  - `Client`: Payment-enabled HTTP client
- **`http`**: Low-level HTTP client (feature: `http-client`)
- **`keystore`**: Encrypted key storage (feature: `keystore`)
- **`config`**: Configuration types
- **`network`**: Network definitions and metadata
- **`currency`**: Token/currency definitions
- **`signer`**: Transaction signing abstractions
- **`error`**: Error types

## Architecture

This library follows a modular architecture inspired by [Alloy](https://alloy.rs):

1. **Protocol Layer**: Core types that map directly to the Web Payment Auth spec
2. **Provider Layer**: Blockchain-specific payment implementations
3. **Client Layer**: High-level convenience API for making paid requests

You can use any layer independently:
- Just protocol types for custom implementations
- Protocol + providers for non-HTTP payment flows
- Full stack for complete HTTP + payment solution

## Supported Networks

### EVM Networks (feature: `evm`)
- Ethereum (mainnet, sepolia)
- Base (mainnet, sepolia)
- Polygon
- Arbitrum
- Optimism
- Avalanche
- Tempo (moderato testnet)

### Solana Networks (feature: `solana`)
- Solana (mainnet-beta, devnet)

Custom networks can be configured via the `Config` type.

## Examples

See the [examples](../examples) directory for more:
- [Basic HTTP request with payment](../examples/basic_request.rs)
- [Custom provider implementation](../examples/custom_provider.rs)
- [Protocol-only usage](../examples/protocol_only.rs)

## CLI Tool

This library powers the [`purl` CLI tool](https://github.com/brendanjryan/purl), a curl-like command for payment-enabled HTTP requests.

```bash
cargo install purl-cli
purl https://api.example.com/premium-data
```

## Contributing

Contributions are welcome! Please see the [main repository](https://github.com/brendanjryan/purl) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
