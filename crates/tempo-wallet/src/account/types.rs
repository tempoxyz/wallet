//! Wallet account data types (balances, keys, spending limits).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TokenBalance {
    pub symbol: String,
    pub currency: String,
    pub balance: String,
}

/// Spending limit for the key's authorized token.
#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    pub unlimited: bool,
    pub limit: Option<String>,
    pub remaining: Option<String>,
    pub spent: Option<String>,
}

/// Key details for JSON output.
#[derive(Debug, Serialize)]
pub(crate) struct KeyInfo {
    pub label: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
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
    pub(crate) fn new(keys: Vec<KeyInfo>) -> Self {
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
