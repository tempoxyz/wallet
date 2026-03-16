//! Wallet account data types (balances, keys, spending limits).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TokenBalance {
    pub symbol: String,
    pub token: String,
    pub balance: String,
}

/// Spending limit for the key's authorized token.
#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    pub unlimited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent: Option<String>,
}

/// Key details for JSON output.
#[derive(Debug, Serialize)]
pub(crate) struct KeyInfo {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_address: Option<String>,
    #[serde(skip)]
    pub wallet_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spending_limit: Option<SpendingLimitInfo>,
    /// Key expiry as an ISO-8601 UTC timestamp (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct KeysResponse {
    pub keys: Vec<KeyInfo>,
    total: usize,
}

impl KeysResponse {
    pub(crate) const fn new(keys: Vec<KeyInfo>) -> Self {
        let total = keys.len();
        Self { keys, total }
    }
}

/// Balance breakdown with locked/available/total.
pub(crate) struct BalanceBreakdown {
    pub total: String,
    pub locked: String,
    pub available: String,
    pub session_count: usize,
}

/// Nested balance object for structured JSON output.
#[derive(Debug, Default, Serialize)]
pub(crate) struct BalanceInfo {
    pub total: String,
    pub locked: String,
    pub available: String,
    pub active_sessions: usize,
    pub symbol: String,
}
