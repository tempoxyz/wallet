//! purl - Library for implementing the Web Payment Auth protocol
//!
//! This library provides the core functionality for handling HTTP 402 payments,
//! including payment provider abstractions, HTTP client, and configuration management.
//!
//! # Feature Flags
//!
//! - `web-payment-auth`: Core protocol types (minimal dependencies)
//! - `runtime`: Async runtime support (tokio, async-trait)
//! - `utils`: Encoding and utility functions (bs58, hex, base64, rand)
//! - `config`: Configuration file support (toml, regex)
//! - `http-client`: HTTP client using reqwest
//! - `client`: High-level Client API (requires http-client and web-payment-auth)
//! - `keystore`: Encrypted keystore management
//! - `evm`: EVM provider support (Tempo, Ethereum, etc.)
//! - `full`: All features enabled
//! - `tower-middleware`: Tower middleware for hyper, axum, tonic, etc.
//! - `reqwest-middleware`: Reqwest middleware integration
//! - `middleware`: All middleware features

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod config;
pub mod constants;
pub mod crypto;
pub mod currency;
pub mod error;
pub mod explorer;
pub mod money;
pub mod network;
pub mod path_validation;
pub mod payment_provider;
pub mod protocol;
pub mod signer;
pub mod types;
pub mod utils;

#[cfg(feature = "http-client")]
pub mod http;

#[cfg(feature = "keystore")]
pub mod keystore;

#[cfg(feature = "evm")]
pub mod providers;

#[cfg(feature = "client")]
pub mod client;

#[cfg(any(feature = "tower-middleware", feature = "reqwest-middleware"))]
pub mod middleware;

pub mod prelude;

pub use config::{Config, ConfigBuilder};
pub use error::{PurlError, Result, ResultExt, SigningContext};

pub use config::{CustomNetwork, CustomToken, EvmConfig, PaymentMethod, WalletConfig};
pub use currency::{currencies, Currency};
pub use explorer::{ExplorerConfig, ExplorerType};
pub use money::{format_u256_trimmed, format_u256_with_decimals, Money, TokenId};
pub use network::{evm_chain_ids, networks, ChainType, GasConfig, Network, NetworkInfo};
pub use path_validation::validate_path;
pub use payment_provider::{
    AddressProvider, BalanceProvider, BuiltinProvider, NetworkBalance, PaymentProvider, Provider,
    PROVIDER_REGISTRY,
};
pub use types::TokenAmount;

// Re-export alloy primitives for convenient access
#[cfg(feature = "evm")]
pub use alloy::primitives::{Address, U256};

#[cfg(feature = "client")]
pub use client::{Client, ClientBuilder, Configured, PaymentResult, Unconfigured};

#[cfg(feature = "http-client")]
pub use http::{
    find_header, has_header, parse_headers, HttpClient, HttpClientBuilder, HttpMethod, HttpResponse,
};

#[cfg(feature = "blocking")]
pub use http::blocking;

// Middleware exports
#[cfg(any(feature = "tower-middleware", feature = "reqwest-middleware"))]
pub use middleware::{PaymentHandler, PaymentHandlerConfig};

#[cfg(feature = "tower-middleware")]
pub use middleware::{PaymentLayer, PaymentService};

#[cfg(feature = "reqwest-middleware")]
pub use middleware::PaymentMiddleware;
