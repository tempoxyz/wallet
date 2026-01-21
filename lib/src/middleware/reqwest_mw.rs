//! Reqwest middleware for automatic payment handling.
//!
//! This module provides a middleware implementation for the `reqwest-middleware`
//! crate that automatically handles HTTP 402 Payment Required responses using
//! the Web Payment Auth protocol.
//!
//! # Overview
//!
//! The reqwest middleware integrates with the `reqwest-middleware` ecosystem,
//! allowing you to add payment handling to reqwest clients through the
//! middleware pattern.
//!
//! # Example
//!
//! ```ignore
//! use purl::middleware::{PaymentMiddleware, PaymentHandlerConfig};
//! use reqwest_middleware::ClientBuilder;
//!
//! let config = purl::Config::load()?;
//! let payment_config = PaymentHandlerConfig::new(config)
//!     .max_amount(1_000_000u128)
//!     .allowed_networks(&["tempo-moderato"]);
//!
//! let client = ClientBuilder::new(reqwest::Client::new())
//!     .with(PaymentMiddleware::new(payment_config))
//!     .build();
//!
//! // Now all requests through `client` will automatically handle 402 responses
//! let response = client.get("https://api.example.com/paid-resource")
//!     .send()
//!     .await?;
//! ```

use std::sync::Arc;

use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};

use super::{PaymentHandler, PaymentHandlerConfig};
use crate::config::Config;
use crate::error::PurlError;
use crate::protocol::web::AUTHORIZATION_HEADER;

/// Reqwest middleware that handles payment-required responses.
///
/// This middleware intercepts 402 Payment Required responses and automatically
/// negotiates and submits payments before retrying the request.
///
/// # Example
///
/// ```ignore
/// use purl::middleware::{PaymentMiddleware, PaymentHandlerConfig};
/// use reqwest_middleware::ClientBuilder;
///
/// let config = purl::Config::load()?;
/// let client = ClientBuilder::new(reqwest::Client::new())
///     .with(PaymentMiddleware::new(PaymentHandlerConfig::new(config)))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct PaymentMiddleware {
    handler: Arc<PaymentHandler>,
}

impl PaymentMiddleware {
    /// Create a new payment middleware with the given configuration.
    pub fn new(config: PaymentHandlerConfig) -> Self {
        Self {
            handler: Arc::new(PaymentHandler::new(config)),
        }
    }

    /// Create a new payment middleware from a purl Config.
    ///
    /// This is a convenience method that creates a default `PaymentHandlerConfig`.
    pub fn from_config(config: Config) -> Self {
        Self::new(PaymentHandlerConfig::new(config))
    }

    /// Set the maximum amount (in token base units) willing to pay.
    #[must_use]
    pub fn max_amount(self, amount: u128) -> Self {
        Self {
            handler: Arc::new(PaymentHandler::new(
                self.handler.config().clone().max_amount(amount),
            )),
        }
    }

    /// Restrict payments to only these networks.
    #[must_use]
    pub fn allowed_networks(self, networks: &[&str]) -> Self {
        Self {
            handler: Arc::new(PaymentHandler::new(
                self.handler.config().clone().allowed_networks(networks),
            )),
        }
    }

    /// Enable or disable dry-run mode.
    #[must_use]
    pub fn dry_run(self, dry_run: bool) -> Self {
        Self {
            handler: Arc::new(PaymentHandler::new(
                self.handler.config().clone().dry_run(dry_run),
            )),
        }
    }

    /// Get a reference to the payment handler.
    pub fn handler(&self) -> &PaymentHandler {
        &self.handler
    }
}

/// Error type for payment middleware.
#[derive(Debug)]
pub struct PaymentMiddlewareError(pub PurlError);

impl std::fmt::Display for PaymentMiddlewareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "payment middleware error: {}", self.0)
    }
}

impl std::error::Error for PaymentMiddlewareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<PurlError> for PaymentMiddlewareError {
    fn from(err: PurlError) -> Self {
        Self(err)
    }
}

#[async_trait::async_trait]
impl Middleware for PaymentMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        // Clone the request for potential retry
        let cloned_req = req.try_clone();

        // Make the initial request
        let response = next.clone().run(req, extensions).await?;

        // Check if payment is required
        let status = response.status().as_u16();
        if !self.handler.requires_payment(status) {
            return Ok(response);
        }

        // Get WWW-Authenticate header
        let www_auth = response
            .headers()
            .get(PaymentHandler::www_authenticate_header())
            .and_then(|v| v.to_str().ok());

        let www_auth = match www_auth {
            Some(header) => header,
            None => {
                // No WWW-Authenticate header, return original response
                return Ok(response);
            }
        };

        // Check if it's a payment challenge
        if !self.handler.is_payment_challenge(Some(www_auth)) {
            // Not a payment challenge, return the original 402 response
            return Ok(response);
        }

        // Parse and validate challenge
        let challenge = self
            .handler
            .parse_challenge(www_auth)
            .map_err(|e| reqwest_middleware::Error::Middleware(PaymentMiddlewareError(e).into()))?;

        self.handler
            .validate_challenge(&challenge)
            .map_err(|e| reqwest_middleware::Error::Middleware(PaymentMiddlewareError(e).into()))?;

        // Check dry-run mode
        if self.handler.is_dry_run() {
            return Err(reqwest_middleware::Error::Middleware(
                PaymentMiddlewareError(PurlError::InvalidChallenge(
                    "Dry-run mode enabled - payment not executed".to_string(),
                ))
                .into(),
            ));
        }

        // Create payment credential
        let credential = self
            .handler
            .create_credential(&challenge)
            .await
            .map_err(|e| reqwest_middleware::Error::Middleware(PaymentMiddlewareError(e).into()))?;

        // Format authorization header
        let auth_header = self
            .handler
            .format_authorization(&credential)
            .map_err(|e| reqwest_middleware::Error::Middleware(PaymentMiddlewareError(e).into()))?;

        // Get the cloned request for retry
        let mut retry_req = cloned_req.ok_or_else(|| {
            reqwest_middleware::Error::Middleware(
                PaymentMiddlewareError(PurlError::Http(
                    "Failed to clone request for retry - request body may not be clonable"
                        .to_string(),
                ))
                .into(),
            )
        })?;

        // Add authorization header
        retry_req.headers_mut().insert(
            AUTHORIZATION_HEADER,
            auth_header.parse().map_err(|_| {
                reqwest_middleware::Error::Middleware(
                    PaymentMiddlewareError(PurlError::Http(
                        "Invalid authorization header value".to_string(),
                    ))
                    .into(),
                )
            })?,
        );

        // Retry the request with payment credential
        next.run(retry_req, extensions).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_middleware_new() {
        let config = Config::default();
        let _middleware = PaymentMiddleware::from_config(config);
    }

    #[test]
    fn test_payment_middleware_builder() {
        let config = Config::default();
        let middleware = PaymentMiddleware::from_config(config)
            .max_amount(1_000_000u128)
            .allowed_networks(&["tempo-moderato"])
            .dry_run(true);

        assert!(middleware.handler.is_dry_run());
    }

    #[test]
    fn test_payment_middleware_error_display() {
        let err = PaymentMiddlewareError(PurlError::Http("test error".to_string()));
        assert!(err.to_string().contains("payment middleware error"));
        assert!(err.to_string().contains("test error"));
    }
}
