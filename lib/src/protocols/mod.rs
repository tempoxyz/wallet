//! Payment protocol implementations
//!
//! This module contains implementations for different HTTP payment protocols:
//!
//! - [`web`] - Web Payment Auth (IETF draft-ietf-httpauth-payment)
//! - [`x402`] - x402 protocol for HTTP 402 payments

pub mod web;
pub mod x402;
