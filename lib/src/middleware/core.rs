//! Core payment handler logic for middleware implementations.
//!
//! This module provides HTTP-client-agnostic payment handling that can be used
//! by various middleware implementations (Tower, reqwest-middleware, etc.).

use std::sync::Arc;

use crate::config::Config;
use crate::error::{PurlError, Result};
use crate::payment_provider::PROVIDER_REGISTRY;
use crate::protocol::web::{
    format_authorization, parse_www_authenticate, ChargeRequest, PaymentChallenge,
    PaymentCredential, PaymentProtocol, WWW_AUTHENTICATE_HEADER,
};

/// Configuration for payment handler middleware.
///
/// This configuration controls how payment challenges are validated and processed.
#[derive(Debug, Clone)]
pub struct PaymentHandlerConfig {
    /// Purl configuration containing wallet and network settings.
    pub config: Arc<Config>,

    /// Maximum amount (in token base units) willing to pay.
    ///
    /// If a payment request exceeds this amount, the handler will return an error.
    pub max_amount: Option<String>,

    /// Networks to allow for payments.
    ///
    /// If non-empty, only payment challenges for these networks will be processed.
    /// Empty means all supported networks are allowed.
    pub allowed_networks: Vec<String>,

    /// Enable dry-run mode.
    ///
    /// In dry-run mode, challenges are validated but no actual payments are made.
    pub dry_run: bool,
}

impl PaymentHandlerConfig {
    /// Create a new payment handler configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            max_amount: None,
            allowed_networks: Vec::new(),
            dry_run: false,
        }
    }

    /// Set the maximum amount willing to pay.
    #[must_use]
    pub fn max_amount(mut self, amount: impl Into<String>) -> Self {
        self.max_amount = Some(amount.into());
        self
    }

    /// Restrict payments to only these networks.
    #[must_use]
    pub fn allowed_networks(mut self, networks: &[&str]) -> Self {
        self.allowed_networks = networks.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Enable or disable dry-run mode.
    #[must_use]
    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }
}

/// HTTP-client-agnostic payment handler.
///
/// This handler provides the core payment processing logic that can be used
/// by various middleware implementations. It handles:
///
/// - Detecting payment requirements from HTTP status codes and headers
/// - Parsing payment challenges from WWW-Authenticate headers
/// - Validating challenges against configured limits
/// - Creating payment credentials using the appropriate provider
/// - Formatting Authorization headers for retry requests
///
/// # Example
///
/// ```ignore
/// use purl::middleware::{PaymentHandler, PaymentHandlerConfig};
///
/// let config = purl::Config::load()?;
/// let handler = PaymentHandler::new(PaymentHandlerConfig::new(config)
///     .max_amount("1000000")
///     .allowed_networks(&["base", "tempo"]));
///
/// // Check if response requires payment
/// if handler.requires_payment(status_code) {
///     // Parse the challenge
///     let challenge = handler.parse_challenge(www_auth_header)?;
///
///     // Validate against our limits
///     handler.validate_challenge(&challenge)?;
///
///     // Create payment credential
///     let credential = handler.create_credential(&challenge).await?;
///
///     // Format the Authorization header
///     let auth_header = handler.format_authorization(&credential)?;
///
///     // Retry request with auth_header...
/// }
/// ```
#[derive(Debug, Clone)]
pub struct PaymentHandler {
    config: PaymentHandlerConfig,
}

impl PaymentHandler {
    /// Create a new payment handler with the given configuration.
    pub fn new(config: PaymentHandlerConfig) -> Self {
        Self { config }
    }

    /// Check if the HTTP status code indicates a payment is required.
    ///
    /// Returns `true` for HTTP 402 Payment Required status.
    #[inline]
    pub fn requires_payment(&self, status: u16) -> bool {
        status == 402
    }

    /// Check if the WWW-Authenticate header indicates Web Payment Auth protocol.
    ///
    /// Returns `true` if the header starts with "Payment " (case-insensitive).
    pub fn is_payment_challenge(&self, www_authenticate: Option<&str>) -> bool {
        PaymentProtocol::detect(www_authenticate).is_some()
    }

    /// Parse a payment challenge from a WWW-Authenticate header value.
    ///
    /// # Errors
    ///
    /// Returns an error if the header is malformed or missing required fields.
    pub fn parse_challenge(&self, www_authenticate: &str) -> Result<PaymentChallenge> {
        parse_www_authenticate(www_authenticate)
    }

    /// Validate a payment challenge against the handler's configuration.
    ///
    /// This validates:
    /// - The payment method is supported
    /// - The payment intent is supported (only 'charge' currently)
    /// - The network is in the allowed networks list (if configured)
    /// - The amount does not exceed the maximum (if configured)
    ///
    /// # Errors
    ///
    /// Returns an error if any validation fails.
    pub fn validate_challenge(&self, challenge: &PaymentChallenge) -> Result<()> {
        // Validate basic challenge requirements
        challenge.validate()?;

        // Get the network name
        let network_name = challenge.network_name()?;

        // Check allowed networks
        if !self.config.allowed_networks.is_empty()
            && !self
                .config
                .allowed_networks
                .contains(&network_name.to_string())
        {
            return Err(PurlError::NoCompatibleMethod {
                networks: vec![network_name.to_string()],
            });
        }

        // Validate amount if max is configured
        if let Some(ref max_amount) = self.config.max_amount {
            let charge_req: ChargeRequest = serde_json::from_value(challenge.request.clone())
                .map_err(|e| {
                    PurlError::InvalidChallenge(format!("Invalid charge request: {}", e))
                })?;

            charge_req.validate_max_amount(max_amount)?;
        }

        Ok(())
    }

    /// Create a payment credential for the given challenge.
    ///
    /// This uses the appropriate payment provider based on the challenge's
    /// payment method to sign the payment transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No provider is found for the network
    /// - The payment cannot be created (signing fails, etc.)
    pub async fn create_credential(
        &self,
        challenge: &PaymentChallenge,
    ) -> Result<PaymentCredential> {
        if self.config.dry_run {
            return Err(PurlError::InvalidChallenge(
                "Cannot create credential in dry-run mode".to_string(),
            ));
        }

        let network_name = challenge.network_name()?;

        let provider = PROVIDER_REGISTRY
            .find_provider(network_name)
            .ok_or_else(|| PurlError::ProviderNotFound(network_name.to_string()))?;

        provider
            .create_web_payment(challenge, &self.config.config)
            .await
    }

    /// Format a payment credential as an Authorization header value.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential cannot be encoded.
    pub fn format_authorization(&self, credential: &PaymentCredential) -> Result<String> {
        format_authorization(credential)
    }

    /// Get the underlying configuration.
    pub fn config(&self) -> &PaymentHandlerConfig {
        &self.config
    }

    /// Check if dry-run mode is enabled.
    pub fn is_dry_run(&self) -> bool {
        self.config.dry_run
    }

    /// Get the WWW-Authenticate header name (lowercase).
    pub fn www_authenticate_header() -> &'static str {
        WWW_AUTHENTICATE_HEADER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EvmConfig;

    /// Test EVM private key (DO NOT use in production)
    const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

    fn test_config() -> Config {
        Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_payment_handler_config_new() {
        let config = test_config();
        let handler_config = PaymentHandlerConfig::new(config);

        assert!(handler_config.max_amount.is_none());
        assert!(handler_config.allowed_networks.is_empty());
        assert!(!handler_config.dry_run);
    }

    #[test]
    fn test_payment_handler_config_builder() {
        let config = test_config();
        let handler_config = PaymentHandlerConfig::new(config)
            .max_amount("1000000")
            .allowed_networks(&["base", "tempo"])
            .dry_run(true);

        assert_eq!(handler_config.max_amount, Some("1000000".to_string()));
        assert_eq!(handler_config.allowed_networks.len(), 2);
        assert!(handler_config
            .allowed_networks
            .contains(&"base".to_string()));
        assert!(handler_config
            .allowed_networks
            .contains(&"tempo".to_string()));
        assert!(handler_config.dry_run);
    }

    #[test]
    fn test_payment_handler_requires_payment() {
        let config = test_config();
        let handler = PaymentHandler::new(PaymentHandlerConfig::new(config));

        assert!(handler.requires_payment(402));
        assert!(!handler.requires_payment(200));
        assert!(!handler.requires_payment(401));
        assert!(!handler.requires_payment(403));
        assert!(!handler.requires_payment(404));
        assert!(!handler.requires_payment(500));
    }

    #[test]
    fn test_payment_handler_is_payment_challenge() {
        let config = test_config();
        let handler = PaymentHandler::new(PaymentHandlerConfig::new(config));

        assert!(handler.is_payment_challenge(Some("Payment id=\"abc\"")));
        assert!(handler.is_payment_challenge(Some("payment id=\"abc\"")));
        assert!(handler.is_payment_challenge(Some("PAYMENT id=\"abc\"")));
        assert!(!handler.is_payment_challenge(Some("Bearer token")));
        assert!(!handler.is_payment_challenge(Some("Basic dXNlcjpwYXNz")));
        assert!(!handler.is_payment_challenge(None));
    }

    #[test]
    fn test_payment_handler_is_dry_run() {
        let config = test_config();

        let handler = PaymentHandler::new(PaymentHandlerConfig::new(config.clone()).dry_run(true));
        assert!(handler.is_dry_run());

        let handler = PaymentHandler::new(PaymentHandlerConfig::new(config).dry_run(false));
        assert!(!handler.is_dry_run());
    }

    #[test]
    fn test_www_authenticate_header() {
        assert_eq!(
            PaymentHandler::www_authenticate_header(),
            "www-authenticate"
        );
    }
}
