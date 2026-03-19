//! Serde types for the x402 payment protocol (v1 and v2).

use serde::{Deserialize, Serialize};

/// Top-level x402 payment challenge decoded from the `PAYMENT-REQUIRED` header.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402PaymentRequired {
    pub(crate) x402_version: u32,
    pub(crate) accepts: Vec<X402PaymentOption>,
    #[allow(dead_code)]
    pub(crate) error: Option<String>,
    /// v2 only — structured resource object. Absent in v1.
    pub(crate) resource: Option<serde_json::Value>,
}

/// A single payment option from the `accepts[]` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402PaymentOption {
    pub(crate) scheme: String,
    pub(crate) network: String,
    /// v2 field. v1 uses `maxAmountRequired` instead.
    pub(crate) amount: Option<String>,
    /// v1 field. Falls back when `amount` is absent.
    pub(crate) max_amount_required: Option<String>,
    pub(crate) asset: String,
    pub(crate) pay_to: String,
    pub(crate) max_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) extra: X402Extra,
}

impl X402PaymentOption {
    /// Resolved amount: prefers `amount` (v2), falls back to `maxAmountRequired` (v1).
    pub(crate) fn resolved_amount(&self) -> Option<&str> {
        self.amount
            .as_deref()
            .or(self.max_amount_required.as_deref())
    }
}

/// Extra fields on a payment option (EIP-712 domain info, asset transfer method).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402Extra {
    pub(crate) name: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) asset_transfer_method: Option<String>,
}

// ---------------------------------------------------------------------------
// PAYMENT-SIGNATURE payload types
// ---------------------------------------------------------------------------

/// v1 payload sent in the `PAYMENT-SIGNATURE` header.
///
/// v1 uses top-level `scheme` + `network` fields.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402PaymentPayloadV1 {
    pub(crate) x402_version: u32,
    pub(crate) scheme: String,
    pub(crate) network: String,
    pub(crate) payload: X402SignedPayload,
}

/// v2 payload sent in the `PAYMENT-SIGNATURE` header.
///
/// v2 uses `resource` + `accepted` fields.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402PaymentPayloadV2 {
    pub(crate) x402_version: u32,
    pub(crate) resource: serde_json::Value,
    pub(crate) accepted: serde_json::Value,
    pub(crate) payload: X402SignedPayload,
}

/// The `payload` field inside the payment payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402SignedPayload {
    pub(crate) signature: String,
    pub(crate) authorization: X402Authorization,
}

/// EIP-3009 authorization parameters echoed in the payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct X402Authorization {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) value: String,
    pub(crate) valid_after: String,
    pub(crate) valid_before: String,
    pub(crate) nonce: String,
}
