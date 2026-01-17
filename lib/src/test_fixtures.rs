//! Shared test fixtures and utilities for purl tests
//!
//! This module provides common test constants, helpers, and fixtures
//! used across lib and cli tests to avoid duplication.

use crate::http::HttpResponse;
use std::collections::HashMap;

// =============================================================================
// Test Private Keys
// =============================================================================

/// Valid test EVM private key (64 hex characters, DO NOT use in production)
pub const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

/// Valid test Solana private key (base58 encoded, DO NOT use in production)
pub const TEST_SOLANA_KEY: &str =
    "4Z7cXSyeFR8wNGMVXUE1TwtKn5D5Vu7FzEv69dokLv7KrQk7h6pu4LF8ZRR9yQBhc7uSM6RTTZtU1fmaxiNrxXrs";

// =============================================================================
// Test Addresses
// =============================================================================

/// Generic test EVM address
pub const TEST_EVM_ADDRESS: &str = "0x1234567890123456789012345678901234567890";

/// Generic test recipient EVM address
pub const TEST_EVM_RECIPIENT: &str = "0xabcdef1234567890abcdef1234567890abcdef12";

/// Generic test Solana address (system program)
pub const TEST_SOLANA_ADDRESS: &str = "11111111111111111111111111111111";

// =============================================================================
// Token Addresses
// =============================================================================

/// USDC on Base mainnet
pub const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

/// USDC on Ethereum mainnet
pub const USDC_ETHEREUM: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";

/// USDC on Solana mainnet
pub const USDC_SOLANA: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// SPL Token Program ID
pub const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// SPL Token 2022 Program ID
pub const SPL_TOKEN_2022_PROGRAM: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

// =============================================================================
// Test Helpers
// =============================================================================

/// Create a test HTTP response with the given parameters
pub fn create_test_response(
    status_code: u32,
    headers: HashMap<String, String>,
    body: &[u8],
) -> HttpResponse {
    HttpResponse {
        status_code,
        headers,
        body: body.to_vec(),
    }
}

/// Create a 402 Payment Required response with empty body
pub fn create_402_response() -> HttpResponse {
    create_test_response(402, HashMap::new(), b"{}")
}

/// Create a 200 OK response with optional body
pub fn create_200_response(body: &[u8]) -> HttpResponse {
    create_test_response(200, HashMap::new(), body)
}

/// Create a response with custom headers
pub fn create_response_with_headers(
    status_code: u32,
    headers: Vec<(&str, &str)>,
    body: &[u8],
) -> HttpResponse {
    let headers: HashMap<String, String> = headers
        .into_iter()
        .map(|(k, v)| (k.to_lowercase(), v.to_string()))
        .collect();
    create_test_response(status_code, headers, body)
}

// =============================================================================
// JSON Fixture Builders
// =============================================================================

/// Builder for creating x402 v1 payment requirement JSON
#[derive(Default)]
pub struct PaymentRequirementBuilder {
    network: String,
    amount: String,
    pay_to: String,
    asset: String,
    scheme: String,
    resource: String,
    description: String,
    mime_type: String,
    max_timeout: u64,
    extra: Option<serde_json::Value>,
}

impl PaymentRequirementBuilder {
    pub fn new() -> Self {
        Self {
            scheme: "exact".to_string(),
            resource: "https://api.example.com/data".to_string(),
            description: "Test payment".to_string(),
            mime_type: "application/json".to_string(),
            max_timeout: 300u64,
            ..Default::default()
        }
    }

    /// Create a builder pre-configured for EVM (Base) payments
    pub fn evm() -> Self {
        Self::new()
            .network("base")
            .pay_to(TEST_EVM_ADDRESS)
            .asset(USDC_BASE)
            .extra(serde_json::json!({
                "name": "USD Coin",
                "version": "2"
            }))
    }

    /// Create a builder pre-configured for Solana payments
    pub fn solana() -> Self {
        Self::new()
            .network("solana")
            .pay_to(TEST_SOLANA_ADDRESS)
            .asset(USDC_SOLANA)
            .extra(serde_json::json!({
                "feePayer": TEST_SOLANA_ADDRESS
            }))
    }

    pub fn network(mut self, network: &str) -> Self {
        self.network = network.to_string();
        self
    }

    pub fn amount(mut self, amount: &str) -> Self {
        self.amount = amount.to_string();
        self
    }

    pub fn pay_to(mut self, address: &str) -> Self {
        self.pay_to = address.to_string();
        self
    }

    pub fn asset(mut self, asset: &str) -> Self {
        self.asset = asset.to_string();
        self
    }

    pub fn extra(mut self, extra: serde_json::Value) -> Self {
        self.extra = Some(extra);
        self
    }

    /// Build the JSON string for this payment requirement
    pub fn build_json(&self) -> String {
        let extra_str = self
            .extra
            .as_ref()
            .map(|e| format!(r#","extra": {}"#, e))
            .unwrap_or_default();

        format!(
            r#"{{
                "x402Version": 1,
                "error": "Payment Required",
                "accepts": [{{
                    "scheme": "{}",
                    "network": "{}",
                    "maxAmountRequired": "{}",
                    "resource": "{}",
                    "description": "{}",
                    "mimeType": "{}",
                    "payTo": "{}",
                    "maxTimeoutSeconds": {},
                    "asset": "{}"{}
                }}]
            }}"#,
            self.scheme,
            self.network,
            self.amount,
            self.resource,
            self.description,
            self.mime_type,
            self.pay_to,
            self.max_timeout,
            self.asset,
            extra_str
        )
    }

    /// Build the payment requirement struct directly
    pub fn build(&self) -> crate::x402::v1::PaymentRequirements {
        crate::x402::v1::PaymentRequirements {
            scheme: self.scheme.clone(),
            network: self.network.clone(),
            max_amount_required: self.amount.clone(),
            resource: self.resource.clone(),
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            pay_to: self.pay_to.clone(),
            max_timeout_seconds: self.max_timeout,
            asset: self.asset.clone(),
            extra: self.extra.clone(),
            output_schema: None,
        }
    }
}

// =============================================================================
// Config Builders for Tests
// =============================================================================

/// Create a test EVM config with the default test key
pub fn test_evm_config() -> crate::config::EvmConfig {
    crate::config::EvmConfig {
        keystore: None,
        private_key: Some(TEST_EVM_KEY.to_string()),
    }
}

/// Create a test Solana config with the default test key
pub fn test_solana_config() -> crate::config::SolanaConfig {
    crate::config::SolanaConfig {
        keystore: None,
        private_key: Some(TEST_SOLANA_KEY.to_string()),
    }
}

/// Create a test config with only EVM enabled
pub fn test_config_evm_only() -> crate::config::Config {
    crate::config::Config {
        evm: Some(test_evm_config()),
        solana: None,
        ..Default::default()
    }
}

/// Create a test config with only Solana enabled
pub fn test_config_solana_only() -> crate::config::Config {
    crate::config::Config {
        evm: None,
        solana: Some(test_solana_config()),
        ..Default::default()
    }
}

/// Create a test config with both EVM and Solana enabled
pub fn test_config_both() -> crate::config::Config {
    crate::config::Config {
        evm: Some(test_evm_config()),
        solana: Some(test_solana_config()),
        ..Default::default()
    }
}

// =============================================================================
// Amount Formatting Utilities (exported for testing)
// =============================================================================

/// Format an atomic amount for display with decimals
///
/// # Arguments
/// * `amount` - The amount in atomic units (e.g., wei, lamports)
/// * `decimals` - Number of decimal places
/// * `symbol` - Token symbol to display
///
/// # Example
/// ```
/// use purl_lib::test_fixtures::format_amount_display;
/// assert_eq!(format_amount_display(1_000_000, 6, "USDC"), "1 USDC");
/// assert_eq!(format_amount_display(1_500_000, 6, "USDC"), "1.500000 USDC");
/// ```
pub fn format_amount_display(amount: u128, decimals: u8, symbol: &str) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = amount / divisor;
    let frac = amount % divisor;

    if frac == 0 {
        format!("{} {}", whole, symbol)
    } else {
        format!(
            "{}.{:0>width$} {}",
            whole,
            frac,
            symbol,
            width = decimals as usize
        )
    }
}

/// Truncate an address for display
///
/// Shows the first 6 and last 4 characters with "..." in between
/// if the address exceeds max_len.
///
/// # Example
/// ```
/// use purl_lib::test_fixtures::truncate_address;
/// assert_eq!(truncate_address("0x1234567890abcdef1234567890abcdef12345678", 20), "0x1234...5678");
/// assert_eq!(truncate_address("short", 20), "short");
/// ```
pub fn truncate_address(addr: &str, max_len: usize) -> String {
    if addr.len() <= max_len {
        addr.to_string()
    } else {
        let prefix = &addr[..6];
        let suffix = &addr[addr.len() - 4..];
        format!("{}...{}", prefix, suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_amount_whole_number() {
        assert_eq!(format_amount_display(1_000_000, 6, "USDC"), "1 USDC");
        assert_eq!(format_amount_display(10_000_000, 6, "USDC"), "10 USDC");
    }

    #[test]
    fn test_format_amount_fractional() {
        assert_eq!(format_amount_display(1_500_000, 6, "USDC"), "1.500000 USDC");
        assert_eq!(format_amount_display(10_000, 6, "USDC"), "0.010000 USDC");
    }

    #[test]
    fn test_format_amount_zero() {
        assert_eq!(format_amount_display(0, 6, "USDC"), "0 USDC");
    }

    #[test]
    fn test_truncate_short_address() {
        assert_eq!(truncate_address("0x1234", 45), "0x1234");
    }

    #[test]
    fn test_truncate_long_evm_address() {
        let addr = "0x1234567890abcdef1234567890abcdef12345678";
        assert_eq!(truncate_address(addr, 20), "0x1234...5678");
    }

    #[test]
    fn test_truncate_empty_address() {
        assert_eq!(truncate_address("", 45), "");
    }

    #[test]
    fn test_payment_requirement_builder_evm() {
        let json = PaymentRequirementBuilder::evm()
            .amount("1000000")
            .build_json();

        assert!(json.contains("\"network\": \"base\""));
        assert!(json.contains("\"maxAmountRequired\": \"1000000\""));
        assert!(json.contains(USDC_BASE));
    }

    #[test]
    fn test_payment_requirement_builder_solana() {
        let json = PaymentRequirementBuilder::solana()
            .amount("500000")
            .build_json();

        assert!(json.contains("\"network\": \"solana\""));
        assert!(json.contains("\"maxAmountRequired\": \"500000\""));
        assert!(json.contains(USDC_SOLANA));
    }
}
