//! HTTP middleware for payment handling.
//!
//! This module provides middleware implementations that can be plugged into
//! various HTTP client ecosystems to automatically handle 402 Payment Required
//! responses using the Web Payment Auth protocol.
//!
//! # Available Middleware
//!
//! - **Tower**: `PaymentLayer` and `PaymentService` for hyper, axum, tonic, etc.
//!   (requires `tower-middleware` feature)
//! - **Reqwest**: `PaymentMiddleware` for reqwest-middleware crate
//!   (requires `reqwest-middleware` feature)
//!
//! # Core Types
//!
//! All middleware implementations are built on [`PaymentHandler`], which provides
//! HTTP-client-agnostic payment handling logic.
//!
//! # Examples
//!
//! ## Tower Middleware
//!
//! ```ignore
//! use purl::middleware::{PaymentLayer, PaymentHandlerConfig};
//! use tower::ServiceBuilder;
//!
//! let config = purl::Config::load()?;
//! let service = ServiceBuilder::new()
//!     .layer(PaymentLayer::new(config).max_amount("1000000"))
//!     .service(hyper_client);
//! ```
//!
//! ## Reqwest Middleware
//!
//! ```ignore
//! use purl::middleware::PaymentMiddleware;
//! use reqwest_middleware::ClientBuilder;
//!
//! let config = purl::Config::load()?;
//! let client = ClientBuilder::new(reqwest::Client::new())
//!     .with(PaymentMiddleware::new(config))
//!     .build();
//! ```

mod core;

#[cfg(feature = "tower-middleware")]
pub mod tower;

#[cfg(feature = "reqwest-middleware")]
pub mod reqwest_mw;

pub use self::core::{PaymentHandler, PaymentHandlerConfig};

#[cfg(feature = "tower-middleware")]
pub use self::tower::{PaymentLayer, PaymentService};

#[cfg(feature = "reqwest-middleware")]
pub use self::reqwest_mw::PaymentMiddleware;
