//! Request-specific analytics event payloads.

use serde::Serialize;
use tempo_common::analytics::EventPayload;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueryStartedPayload {
    pub(crate) url: String,
    pub(crate) method: String,
}
impl EventPayload for QueryStartedPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QuerySuccessPayload {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) status_code: u16,
}
impl EventPayload for QuerySuccessPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueryFailurePayload {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) error: String,
}
impl EventPayload for QueryFailurePayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentStartedPayload {
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) currency: String,
    pub(crate) intent: String,
}
impl EventPayload for PaymentStartedPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentSuccessPayload {
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) currency: String,
    pub(crate) tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    pub(crate) intent: String,
}
impl EventPayload for PaymentSuccessPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentFailurePayload {
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) currency: String,
    pub(crate) error: String,
    pub(crate) intent: String,
}
impl EventPayload for PaymentFailurePayload {}
