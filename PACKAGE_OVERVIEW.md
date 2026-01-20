# Purl Package Overview

## Architecture

Purl follows a library-first architecture inspired by Alloy/Foundry, with clear separation between the core library and CLI tool.

```
┌─────────────────────────────────────────────────────────────┐
│                     purl (library)                          │
│  ┌────────────────────────────────────────────────────┐     │
│  │           Core Protocol Layer                      │     │
│  │  • protocol/    Protocol types & parsing           │     │
│  │  • error/       Error types                        │     │
│  │  • network/     Network definitions                │     │
│  │  • currency/    Token definitions                  │     │
│  └────────────────────────────────────────────────────┘     │
│  ┌────────────────────────────────────────────────────┐     │
│  │           Provider Layer (optional)                │     │
│  │  • payment_provider/  Provider trait               │     │
│  │  • providers/evm/     EVM implementation          │     │
│  │  • signer/            Signing abstractions         │     │
│  └────────────────────────────────────────────────────┘     │
│  ┌────────────────────────────────────────────────────┐     │
│  │           Infrastructure (optional)                │     │
│  │  • http/       HTTP client                         │     │
│  │  • keystore/   Encrypted key storage               │     │
│  │  • config/     Configuration management            │     │
│  └────────────────────────────────────────────────────┘     │
│  ┌────────────────────────────────────────────────────┐     │
│  │           High-Level API (optional)                │     │
│  │  • client/     Client (convenience wrapper)    │     │
│  └────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────┘
                            ↑
                            │ uses as dependency
                            │
┌─────────────────────────────────────────────────────────────┐
│                    purl-cli (binary)                        │
│  • CLI argument parsing (clap)                              │
│  • Command implementations                                  │
│  • Output formatting                                        │
│  • Interactive setup                                        │
└─────────────────────────────────────────────────────────────┘
```

## Library Packages (`purl`)

### Core Modules (Always Available)

#### `protocol/`
**Responsibility**: Web Payment Auth protocol implementation (IETF draft)

**Contents**:
- `protocol/web/types.rs`: Core protocol types
  - `PaymentChallenge`: Challenge from server (WWW-Authenticate header)
  - `PaymentCredential`: Payment credential for retry (Authorization header)
  - `PaymentReceipt`: Receipt from server (Payment-Receipt header)
  - `PaymentMethod`, `PaymentIntent`: Protocol enums
  - `ChargeRequest`, `ChargeResponse`: Payment request/response types
- `protocol/web/parse.rs`: HTTP header parsing
  - `parse_www_authenticate()`: Parse WWW-Authenticate header
  - `parse_receipt()`: Parse Payment-Receipt header
- `protocol/web/encode.rs`: HTTP header encoding
  - `format_authorization()`: Format Authorization header
  - Base64 encoding utilities

**Dependencies**: `serde`, `serde_json`, `base64`, `regex`

**Feature Flag**: Always compiled (part of core)

**Use Case**: Library users who want to implement Web Payment Auth protocol in their own HTTP client

---

#### `error/`
**Responsibility**: Error types and result aliases

**Contents**:
- `PurlError`: Main error enum with variants for:
  - HTTP errors (status codes, headers)
  - Protocol errors (invalid challenges, unsupported methods)
  - Payment errors (amount exceeded, no compatible method)
  - Network errors (provider not found, RPC errors)
  - Crypto errors (invalid keys, signing failures)
  - I/O and parsing errors
- `Result<T>`: Type alias for `std::result::Result<T, PurlError>`

**Dependencies**: `thiserror`, `bs58`, `toml`

**Feature Flag**: Always compiled

**Use Case**: Error handling throughout the library and in consuming applications

---

#### `network/`
**Responsibility**: Network/chain definitions and metadata

**Contents**:
- `Network`: Network identifier enum (Ethereum, Base, etc.)
- `NetworkInfo`: Network metadata (chain ID, RPC URL, native currency)
- `ChainType`: EVM chain type
- `GasConfig`: Gas settings for EVM networks
- `networks`: Module with constants for all supported networks
- `evm_chain_ids()`: Mapping of network names to chain IDs
- Network resolution and CAIP-2 format support

**Dependencies**: `serde`, `serde_json`

**Feature Flag**: Always compiled

**Use Case**: Network configuration, provider routing, chain ID lookups

---

#### `currency/`
**Responsibility**: Token/currency definitions

**Contents**:
- `Currency`: Token metadata (symbol, name, decimals, address)
- `currencies`: Pre-defined currency constants (USDC, ETH, SOL, etc.)
- `format_atomic_trimmed()`: Format atomic units for display
- Token address mappings per network

**Dependencies**: `serde`

**Feature Flag**: Always compiled

**Use Case**: Token amount formatting, currency validation

---

#### `config/`
**Responsibility**: Configuration types and loading

**Contents**:
- `Config`: Main configuration struct
- `EvmConfig`: EVM-specific configuration (keystore path, private key)
- `WalletConfig`: Combined wallet configuration
- `CustomNetwork`: User-defined network configuration
- `CustomToken`: User-defined token configuration
- `PaymentMethod`: Payment method enum (EVM)
- Configuration file loading and validation

**Dependencies**: `serde`, `toml`, `dirs`

**Feature Flag**: Always compiled

**Use Case**: Application configuration, user settings management

---

#### `payment_provider/`
**Responsibility**: Payment provider abstraction and registry

**Contents**:
- `PaymentProvider` trait: Interface for blockchain payment providers
  - `supports_network()`: Check if provider supports a network
  - `get_address()`: Get wallet address
  - `get_balance()`: Query token balance
  - `create_web_payment()`: Create payment credential
- `BuiltinProvider`: Enum for built-in providers (EVM)
- `PaymentProviderRegistry`: Registry for looking up providers by network
- `PROVIDER_REGISTRY`: Global static registry
- `NetworkBalance`: Balance information
- `DryRunInfo`: Dry-run payment information

**Dependencies**: `async-trait`, `tokio`

**Feature Flag**: Always compiled (trait definition)

**Use Case**: Provider abstraction, custom provider implementation

---

#### `signer/`
**Responsibility**: Transaction signing abstractions

**Contents**:
- `Signer` trait (if applicable)
- Private key loading utilities
- Signing utilities

**Dependencies**: `hex`

**Feature Flag**: Always compiled

**Use Case**: Transaction signing, key management

---

#### `crypto/`
**Responsibility**: Cryptographic primitives

**Contents**:
- Key generation utilities
- Random number generation
- Cryptographic utilities

**Dependencies**: `rand`, `hex`

**Feature Flag**: Always compiled

**Use Case**: Key generation, cryptographic operations

---

#### `path_validation/`
**Responsibility**: File path validation

**Contents**:
- `validate_path()`: Validate file paths for security
- Path sanitization utilities

**Dependencies**: None (uses std)

**Feature Flag**: Always compiled

**Use Case**: Secure file path handling

---

#### `constants/`
**Responsibility**: Global constants

**Contents**:
- Default keystore names
- Default configuration paths
- Protocol constants

**Dependencies**: None

**Feature Flag**: Always compiled

**Use Case**: Default values throughout the library

---

#### `utils/`
**Responsibility**: Utility functions

**Contents**:
- Address formatting (`truncate_address()`, `format_eth_address()`)
- String manipulation utilities
- Helper functions

**Dependencies**: `hex`

**Feature Flag**: Always compiled

**Use Case**: Common utility operations

---

### Feature-Gated Modules

#### `http/` (feature: `http-client`)
**Responsibility**: HTTP client with curl bindings

**Contents**:
- `HttpClient`: HTTP client implementation
- `HttpClientBuilder`: Builder for HTTP client
- `HttpMethod`: HTTP method enum (GET, POST, etc.)
- `HttpResponse`: HTTP response struct
- Header parsing utilities (`has_header()`, `find_header()`, `parse_headers()`)
- Curl wrapper with timeout, redirects, custom headers

**Dependencies**: `curl`

**Feature Flag**: `http-client`

**Use Case**: Making HTTP requests, handling 402 responses

---

#### `keystore/` (feature: `keystore`)
**Responsibility**: Encrypted keystore management

**Contents**:
- `keystore/encrypt.rs`: Keystore encryption/decryption
  - `create_keystore()`: Create encrypted keystore
  - `decrypt_keystore()`: Decrypt keystore with password
  - `list_keystores()`: List available keystores
  - `Keystore` struct (Ethereum keystore v3 format)
- `keystore/cache.rs`: Password caching
  - `PasswordCache`: In-memory password cache (5 min TTL)
- `keystore/store.rs`: Keystore storage management

**Dependencies**: `eth-keystore`, `zeroize`, `rpassword`, `dirs`, `once_cell`

**Feature Flag**: `keystore`

**Use Case**: Secure private key storage, password management

---

#### `providers/` (feature: `evm`)
**Responsibility**: Blockchain-specific provider implementations

**Contents**:

**`providers/evm/` (feature: `evm`)**:
- `EvmProvider`: EVM blockchain provider
- `create_web_payment()`: Create EVM payment transaction
- `get_balance()`: Query ERC-20 token balance
- Chain ID resolution
- Gas estimation
- Transaction signing with Alloy

**Dependencies**: `alloy`, `alloy-signer`, `alloy-signer-local`, `eth-keystore`, `tempo-primitives`

**Feature Flag**: `evm`

**Use Case**: Blockchain-specific payment operations

---

#### `client/` (feature: `client`)
**Responsibility**: High-level convenience API

**Contents**:
- `Client`: Main client struct with builder pattern
  - `new()`: Create client from default config
  - `with_config()`: Create client with custom config
  - `get()`, `post()`: HTTP methods
  - `max_amount()`, `allowed_networks()`: Payment constraints
  - `verbose()`, `dry_run()`: Options
- `PaymentResult`: Result enum (Success, WebPaid, DryRun)
- Automatic 402 handling and payment retry
- Protocol negotiation
- Payment verification

**Dependencies**: Requires `http-client` and uses protocol types

**Feature Flag**: `client`

**Use Case**: Making payment-enabled HTTP requests with minimal code

---

## CLI Package (`purl-cli`)

### Command Structure

```
purl [OPTIONS] [URL]              # Make HTTP request
purl init [--force]               # Initialize configuration
purl config [SUBCOMMAND]          # Manage configuration
purl method [SUBCOMMAND]          # Manage payment methods
purl balance [OPTIONS]            # Check balances
purl networks [SUBCOMMAND]        # Manage networks
purl inspect <URL>                # Inspect payment requirements
purl version                      # Show version
purl completions <SHELL>          # Generate shell completions
```

### CLI Modules

#### `main.rs`
**Responsibility**: Entry point, command routing, async runtime

**Contents**:
- Command dispatcher
- Signal handling (Ctrl+C)
- Error formatting and exit codes
- Version display

---

#### `cli.rs`
**Responsibility**: Command-line argument definitions

**Contents**:
- `Cli` struct with clap derive
- Command enums
- Argument parsing
- Environment variable support
- Default values

---

#### `request.rs`
**Responsibility**: HTTP request execution

**Contents**:
- `RequestContext`: Request configuration
- Request building from CLI args
- HTTP method handling
- Header management
- Data/body handling

---

#### `web_payment.rs`
**Responsibility**: Payment flow handling

**Contents**:
- 402 response detection
- Protocol negotiation
- Payment credential creation
- Authorization header formatting
- Payment receipt parsing
- User confirmation prompts

---

#### `output.rs`
**Responsibility**: Output formatting and display

**Contents**:
- Response formatting (text, JSON, YAML)
- Header display
- Color output control
- Verbose mode output
- Config display formatting
- Keystore decryption UI

---

#### `config_commands.rs`
**Responsibility**: Configuration management commands

**Contents**:
- `config` command: Display configuration
- `config get`: Get specific config value
- `config validate`: Validate configuration
- Config formatting (text, JSON, YAML)
- Private key obfuscation

---

#### `wallet_commands.rs`
**Responsibility**: Wallet/keystore management

**Contents**:
- `method list`: List keystores
- `method new`: Create new keystore
- `method import`: Import private key
- `method show`: Display keystore info
- Interactive keystore creation
- Password prompting

---

#### `balance_command.rs`
**Responsibility**: Balance checking

**Contents**:
- `balance` command implementation
- Multi-network balance queries
- Token balance display
- Network filtering
- Parallel balance queries

---

#### `network_commands.rs`
**Responsibility**: Network management

**Contents**:
- `networks list`: List supported networks
- `networks show`: Show network details
- Network filtering by chain type
- Network information display

---

#### `inspect_command.rs`
**Responsibility**: Payment inspection (dry-run)

**Contents**:
- `inspect` command implementation
- Payment requirement analysis
- Challenge display
- Dry-run payment calculation
- Network compatibility checking

---

#### `init.rs`
**Responsibility**: Interactive setup wizard

**Contents**:
- First-time setup flow
- Keystore generation/import
- Configuration file creation
- User prompts and validation
- Platform-specific paths

---

#### `config_utils.rs`
**Responsibility**: Configuration loading utilities

**Contents**:
- Config file discovery
- Config validation
- Config merging (file + CLI overrides)
- Default config generation

---

#### `errors.rs`
**Responsibility**: CLI error formatting

**Contents**:
- Error message formatting
- Contextual help suggestions
- User-friendly error display

---

#### `exit_codes.rs`
**Responsibility**: Exit code definitions

**Contents**:
- Exit code constants
- Error to exit code mapping
- Process exit handling

---

## Feature Flag Matrix

| Feature | Modules Enabled | Dependencies Added | Use Case |
|---------|----------------|-------------------|----------|
| (none) | Core only | Minimal | Protocol types only |
| `web-payment` | Core only | None | Semantic versioning |
| `http-client` | Core + `http/` | curl | HTTP client without payments |
| `client` | Core + `http/` + `client/` | curl | Full HTTP + payment client |
| `keystore` | Core + `keystore/` | eth-keystore, zeroize, etc. | Secure key storage |
| `evm` | Core + `providers/evm/` | alloy, alloy-signer, etc. | EVM payments |
| `full` | All modules | All optional deps | Complete functionality |
| (default) | All except `full` marker | Most deps | Typical usage |

## Dependency Graph

```
Core (always)
  ├─ serde, serde_json, thiserror
  ├─ tokio, async-trait
  ├─ bs58, hex, base64, regex
  └─ toml, rand

http-client (optional)
  └─ curl

keystore (optional)
  ├─ eth-keystore
  ├─ zeroize
  ├─ rpassword
  ├─ dirs
  └─ once_cell

evm (optional)
  ├─ alloy
  ├─ alloy-signer
  ├─ alloy-signer-local
  ├─ eth-keystore
  └─ tempo-primitives

client (optional)
  └─ (requires http-client)

CLI (always, in purl-cli)
  ├─ purl (library)
  ├─ clap, clap_complete
  ├─ anyhow
  ├─ dialoguer
  ├─ colored
  └─ (duplicates some library deps for direct use)
```

## Usage Patterns

### Pattern 1: Protocol-Only Library Consumer
```toml
[dependencies]
purl = { version = "0.1", default-features = false }
```
- Gets: Protocol types, error types, network definitions
- Size: ~50 KB compiled
- Use: Building custom HTTP client with payment support

### Pattern 2: EVM Application
```toml
[dependencies]
purl = { version = "0.1", default-features = false, features = ["evm", "client"] }
```
- Gets: Core + EVM provider + HTTP client + high-level API
- Size: ~2 MB compiled (includes Alloy)
- Use: EVM payment application

### Pattern 3: Full-Featured Application
```toml
[dependencies]
purl = "0.1"  # or features = ["full"]
```
- Gets: Everything
- Size: ~2 MB compiled
- Use: Payment application or CLI tool

### Pattern 4: Custom Provider Implementation
```toml
[dependencies]
purl = { version = "0.1", default-features = false, features = ["web-payment"] }
```
```rust
use purl::provider::PaymentProvider;
// Implement custom provider for new blockchain
```

## Design Principles

1. **Layered Architecture**: Core protocol → Providers → Infrastructure → High-level API
2. **Feature Gates**: Fine-grained control over included functionality
3. **Zero Cost Abstractions**: Unused features don't affect compile time or binary size
4. **Async First**: Built on tokio for async/await support
5. **Type Safety**: Strong typing throughout, minimal `unwrap()` usage
6. **Error Transparency**: Detailed error types with context
7. **Extensibility**: Traits for custom providers and signers
8. **CLI as Consumer**: CLI uses library exactly as external users would

## Future Extensions

Potential additions that fit the architecture:

1. **New Protocol Module**: `protocol/x402/` for HTTP 402 Extensions
2. **New Provider**: `providers/bitcoin/` for Bitcoin Lightning
3. **Transport Layer**: `transport/` for WebSocket/IPC support
4. **Caching Layer**: Response caching, balance caching
5. **Middleware**: Request/response interceptors
6. **Metrics**: Observability and instrumentation
7. **Testing Utilities**: Mock providers, test harness

Each would be a new feature flag maintaining backward compatibility.
