//! Base-specific types and helpers for Web Payment Auth.
//!
//! This module provides Base blockchain-specific implementations.
//! Base uses chain_id 84532 (Base Sepolia testnet).
//!
//! # Types
//!
//! - [`BaseMethodDetails`]: Base-specific method details
//! - [`BaseChargeExt`]: Extension trait for ChargeRequest with Base-specific accessors
//!
//! # Constants
//!
//! - [`CHAIN_ID`]: Base Sepolia chain ID (84532)
//! - [`METHOD_NAME`]: Payment method name ("base")
//!
//! # Examples
//!
//! ```ignore
//! use purl::protocol::intents::ChargeRequest;
//! use purl::protocol::methods::base::{BaseChargeExt, CHAIN_ID};
//!
//! let req: ChargeRequest = challenge.request.decode()?;
//! assert_eq!(CHAIN_ID, 84532);
//! ```

pub mod charge;
pub mod types;

pub use charge::BaseChargeExt;
pub use types::BaseMethodDetails;

/// Base Sepolia testnet chain ID.
pub const CHAIN_ID: u64 = 84532;

/// Payment method name for Base.
pub const METHOD_NAME: &str = "base";

/// Network name for Base Sepolia.
pub const NETWORK_NAME: &str = "base-sepolia";
