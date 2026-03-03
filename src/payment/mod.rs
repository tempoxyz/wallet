//! Payment module containing all payment-related functionality.
//!
//! This module provides payment providers, currencies, and protocol handling.

use mpp::PaymentChallenge;

use crate::network::{Network, NetworkInfo};

pub mod charge;
pub mod dispatch;
pub mod session;

/// Parsed challenge with resolved network, shared by charge and session flows.
pub struct ResolvedChallenge {
    pub challenge: PaymentChallenge,
    pub network: Network,
    pub network_info: NetworkInfo,
}
