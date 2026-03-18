//! Request-specific analytics event payloads.

use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueryStartedPayload {
    pub(crate) url: String,
    pub(crate) method: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QuerySuccessPayload {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) status_code: u16,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueryFailurePayload {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) error: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentStartedPayload {
    pub(crate) url: String,
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) currency: String,
    pub(crate) intent: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentSuccessPayload {
    pub(crate) url: String,
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) channel_id: Option<String>,
    pub(crate) intent: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentFailurePayload {
    pub(crate) url: String,
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) currency: String,
    pub(crate) error: String,
    pub(crate) intent: String,
}
