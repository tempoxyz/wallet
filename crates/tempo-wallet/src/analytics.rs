//! Wallet-specific analytics events and payloads.

use serde::Serialize;
use tempo_common::analytics::{Event, EventPayload};

pub(crate) const LOGIN_STARTED: Event = Event::new("login_started");
pub(crate) const LOGIN_SUCCESS: Event = Event::new("login_success");
pub(crate) const LOGIN_FAILURE: Event = Event::new("login_failure");
pub(crate) const LOGIN_TIMEOUT: Event = Event::new("login_timeout");
pub(crate) const LOGOUT: Event = Event::new("logout");
pub(crate) const KEY_CREATED: Event = Event::new("key_created");
pub(crate) const WHOAMI_VIEWED: Event = Event::new("whoami_viewed");
pub(crate) const CALLBACK_WINDOW_OPENED: Event = Event::new("callback_window_opened");
pub(crate) const CALLBACK_RECEIVED: Event = Event::new("callback_received");
pub(crate) const WALLET_CREATED: Event = Event::new("wallet_created");
pub(crate) const WALLET_FUND_STARTED: Event = Event::new("wallet_fund_started");
pub(crate) const WALLET_FUND_SUCCESS: Event = Event::new("wallet_fund_success");
pub(crate) const WALLET_FUND_FAILURE: Event = Event::new("wallet_fund_failure");

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
