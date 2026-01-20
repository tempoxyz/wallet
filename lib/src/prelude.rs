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
//! - **Configuration**: [`Config`], [`ConfigBuilder`], [`EvmConfig`]
//! - **Client**: [`Client`], [`ClientBuilder`] (with `client` feature)
//! - **Errors**: [`PurlError`], [`Result`]
//! - **Protocol**: [`PaymentChallenge`], [`PaymentCredential`], [`PaymentMethod`]
//! - **Middleware**: [`PaymentHandler`], [`PaymentHandlerConfig`] (with middleware features)

// Core types - always available
pub use crate::config::{Config, ConfigBuilder, EvmConfig};
pub use crate::error::{PurlError, Result, ResultExt, SigningContext};

// Protocol types
pub use crate::protocol::web::{
    ChargeRequest, PaymentChallenge, PaymentCredential, PaymentIntent, PaymentMethod,
    PaymentPayload, PaymentProtocol, PaymentReceipt,
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
