//! purl - Library for implementing the Web Payment Auth protocol
//!
//! This library provides the core functionality for handling HTTP 402 payments,
//! including payment provider abstractions, HTTP client, and configuration management.
//!
//! # Feature Flags
//!
//! - `web-payment`: Core protocol types (included by default)
//! - `http-client`: HTTP client using curl
//! - `client`: High-level Client API (requires http-client)
//! - `keystore`: Encrypted keystore management
//! - `evm`: EVM provider support (Ethereum, Base, etc.)
//! - `solana`: Solana provider support
//! - `full`: All features enabled

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod config;
pub mod constants;
pub mod crypto;
pub mod currency;
pub mod error;
pub mod network;
pub mod path_validation;
pub mod payment_provider;
pub mod protocol;
pub mod signer;
pub mod utils;

#[cfg(feature = "http-client")]
pub mod http;

#[cfg(feature = "keystore")]
pub mod keystore;

#[cfg(any(feature = "evm", feature = "solana"))]
pub mod providers;

#[cfg(feature = "client")]
pub mod client;

pub use config::Config;
pub use error::{PurlError, Result};

pub use config::{
    CustomNetwork, CustomToken, EvmConfig, PaymentMethod, SolanaConfig, WalletConfig,
};
pub use currency::{currencies, Currency};
pub use network::{evm_chain_ids, networks, ChainType, GasConfig, Network, NetworkInfo};
pub use path_validation::validate_path;
pub use payment_provider::{BuiltinProvider, NetworkBalance, PaymentProvider, PROVIDER_REGISTRY};

#[cfg(feature = "client")]
pub use client::Client;

#[cfg(feature = "http-client")]
pub use http::{
    find_header, has_header, parse_headers, HttpClient, HttpClientBuilder, HttpMethod, HttpResponse,
};
