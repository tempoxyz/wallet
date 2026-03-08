//! Wallet-specific analytics event payloads.

use serde::Serialize;
use tempo_common::analytics::EventPayload;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LoginFailurePayload {
    pub(crate) error: String,
}
impl EventPayload for LoginFailurePayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CallbackReceivedPayload {
    pub(crate) duration_secs: u64,
}
impl EventPayload for CallbackReceivedPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WalletCreatedPayload {
    pub(crate) wallet_type: String,
}
impl EventPayload for WalletCreatedPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WalletFundPayload {
    pub(crate) network: String,
    pub(crate) method: String,
}
impl EventPayload for WalletFundPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WalletFundFailurePayload {
    pub(crate) network: String,
    pub(crate) method: String,
    pub(crate) error: String,
}
impl EventPayload for WalletFundFailurePayload {}
