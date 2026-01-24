//! Convenient re-exports for common purl types.
//!
//! This module provides a convenient set of imports for the most commonly used
//! types in the purl library. Instead of importing each type individually,
//! you can use:
//!
//! ```ignore
//! use purl::prelude::*;
//! ```
//!
//! # What's included
//!
//! ## Always Available (Core + Intents)
//!
//! - **Configuration**: [`Config`], [`ConfigBuilder`], [`EvmConfig`]
//! - **Errors**: [`PurlError`], [`Result`]
//! - **Protocol Core**: [`PaymentChallenge`], [`PaymentCredential`], [`PaymentReceipt`]
//! - **Protocol Intents**: [`ChargeRequest`], [`AuthorizeRequest`], [`SubscriptionRequest`]
//!
//! ## Feature-Gated
//!
//! - **Client** (`client` feature): [`Client`], [`ClientBuilder`], [`PaymentResult`]
//! - **Middleware** (`tower-middleware` or `reqwest-middleware` features):
//!   [`PaymentHandler`], [`PaymentHandlerConfig`]
//! - **EVM** (`evm` feature): [`EvmChargeExt`], [`Address`], [`U256`]
//! - **Tempo** (`tempo` feature): [`TempoChargeExt`]

// Core types - always available
pub use crate::config::{Config, ConfigBuilder, EvmConfig};
pub use crate::error::{PurlError, Result, ResultExt, SigningContext};

// New layered protocol types - always available
pub use crate::protocol::core::{
    parse_www_authenticate, parse_www_authenticate_all, Base64UrlJson, ChallengeEcho, IntentName,
    MethodName, PayloadType, PaymentProtocol, ReceiptStatus,
};
pub use crate::protocol::intents::{AuthorizeRequest, SubscriptionRequest};

// Legacy protocol types (backward compatibility) - always available
// These are re-exported from protocol::web
pub use crate::protocol::web::{
    ChargeRequest, PaymentChallenge, PaymentCredential, PaymentIntent, PaymentMethod,
    PaymentPayload, PaymentReceipt,
};

// Client types (feature-gated)
#[cfg(feature = "client")]
pub use crate::client::{Client, ClientBuilder, PaymentResult};

// Middleware types (feature-gated)
#[cfg(any(feature = "tower-middleware", feature = "reqwest-middleware"))]
pub use crate::middleware::{PaymentHandler, PaymentHandlerConfig};

#[cfg(feature = "tower-middleware")]
pub use crate::middleware::{PaymentLayer, PaymentService};

#[cfg(feature = "reqwest-middleware")]
pub use crate::middleware::PaymentMiddleware;

// EVM types (feature-gated)
#[cfg(feature = "evm")]
pub use crate::protocol::methods::evm::{EvmChargeExt, EvmMethodDetails};

#[cfg(feature = "evm")]
pub use alloy::primitives::{Address, U256};

// Tempo types (feature-gated)
#[cfg(feature = "tempo")]
pub use crate::protocol::methods::tempo::{TempoChargeExt, TempoMethodDetails};

// Stripe types (feature-gated)
#[cfg(feature = "stripe")]
pub use crate::protocol::methods::stripe::{StripeChargePayload, StripeMethodDetails};
