//! Tower middleware for automatic payment handling.
//!
//! This module provides a Tower [`Layer`] and [`Service`] implementation that
//! automatically handles HTTP 402 Payment Required responses using the Web
//! Payment Auth protocol.
//!
//! # Overview
//!
//! The Tower middleware integrates with the Tower service ecosystem, allowing
//! you to add payment handling to any Tower-compatible HTTP client (hyper,
//! reqwest with tower, tonic, etc.).
//!
//! # Example
//!
//! ```ignore
//! use purl::middleware::{PaymentLayer, PaymentHandlerConfig};
//! use tower::ServiceBuilder;
//!
//! let config = purl::Config::load()?;
//! let payment_config = PaymentHandlerConfig::new(config)
//!     .max_amount(1_000_000u128)
//!     .allowed_networks(&["base", "tempo"]);
//!
//! let service = ServiceBuilder::new()
//!     .layer(PaymentLayer::new(payment_config))
//!     .service(inner_http_client);
//!
//! // Now all requests through `service` will automatically handle 402 responses
//! let response = service.call(request).await?;
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::{Request, Response};
use http_body::Body;
use tower::{Layer, Service};

use super::{PaymentHandler, PaymentHandlerConfig};
use crate::config::Config;
use crate::error::PurlError;
use crate::protocol::web::AUTHORIZATION_HEADER;

/// Tower [`Layer`] that adds payment handling to an HTTP service.
///
/// This layer wraps an inner service and intercepts 402 Payment Required
/// responses, automatically negotiating and submitting payments before
/// retrying the request.
///
/// # Example
///
/// ```ignore
/// use purl::middleware::{PaymentLayer, PaymentHandlerConfig};
/// use tower::ServiceBuilder;
///
/// let config = purl::Config::load()?;
/// let service = ServiceBuilder::new()
///     .layer(PaymentLayer::new(PaymentHandlerConfig::new(config)))
///     .service(hyper_client);
/// ```
#[derive(Debug, Clone)]
pub struct PaymentLayer {
    handler: Arc<PaymentHandler>,
}

impl PaymentLayer {
    /// Create a new payment layer with the given configuration.
    pub fn new(config: PaymentHandlerConfig) -> Self {
        Self {
            handler: Arc::new(PaymentHandler::new(config)),
        }
    }

    /// Create a new payment layer from a purl Config.
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
}

impl<S> Layer<S> for PaymentLayer {
    type Service = PaymentService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PaymentService {
            inner,
            handler: Arc::clone(&self.handler),
        }
    }
}

/// Tower [`Service`] that handles payment-required responses.
///
/// This service wraps an inner HTTP service and intercepts 402 Payment Required
/// responses. When a 402 is received with a Payment challenge, this service:
///
/// 1. Parses the payment challenge from WWW-Authenticate header
/// 2. Validates the challenge against configured limits
/// 3. Creates a payment credential using the appropriate provider
/// 4. Retries the original request with the Authorization header
///
/// # Type Parameters
///
/// - `S`: The inner service type
#[derive(Debug, Clone)]
pub struct PaymentService<S> {
    inner: S,
    handler: Arc<PaymentHandler>,
}

impl<S> PaymentService<S> {
    /// Create a new payment service wrapping the given inner service.
    pub fn new(inner: S, config: PaymentHandlerConfig) -> Self {
        Self {
            inner,
            handler: Arc::new(PaymentHandler::new(config)),
        }
    }

    /// Get a reference to the inner service.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consume this service, returning the inner service.
    pub fn into_inner(self) -> S {
        self.inner
    }

    /// Get a reference to the payment handler.
    pub fn handler(&self) -> &PaymentHandler {
        &self.handler
    }
}

/// Error type for the payment service.
///
/// This wraps both errors from the inner service and payment-related errors.
#[derive(Debug)]
pub enum PaymentServiceError<E> {
    /// Error from the inner service
    Inner(E),
    /// Payment-related error
    Payment(PurlError),
}

impl<E: std::fmt::Display> std::fmt::Display for PaymentServiceError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inner(e) => write!(f, "inner service error: {}", e),
            Self::Payment(e) => write!(f, "payment error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PaymentServiceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Inner(e) => Some(e),
            Self::Payment(e) => Some(e),
        }
    }
}

impl<E> From<PurlError> for PaymentServiceError<E> {
    fn from(err: PurlError) -> Self {
        Self::Payment(err)
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for PaymentService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: std::error::Error + Send + 'static,
    ReqBody: Body + Clone + Send + 'static,
    ResBody: Body + Send + 'static,
{
    type Response = Response<ResBody>;
    type Error = PaymentServiceError<S::Error>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(PaymentServiceError::Inner)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let handler = Arc::clone(&self.handler);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Make the initial request
            let cloned_request = clone_request(&request);
            let response = inner
                .call(request)
                .await
                .map_err(PaymentServiceError::Inner)?;

            // Check if payment is required
            let status = response.status().as_u16();
            if !handler.requires_payment(status) {
                return Ok(response);
            }

            // Get WWW-Authenticate header
            let www_auth = response
                .headers()
                .get(PaymentHandler::www_authenticate_header())
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    PaymentServiceError::Payment(PurlError::MissingHeader(
                        "WWW-Authenticate".to_string(),
                    ))
                })?;

            // Check if it's a payment challenge
            if !handler.is_payment_challenge(Some(www_auth)) {
                // Not a payment challenge, return the original 402 response
                return Ok(response);
            }

            // Parse and validate challenge
            let challenge = handler.parse_challenge(www_auth)?;
            handler.validate_challenge(&challenge)?;

            // Check dry-run mode
            if handler.is_dry_run() {
                return Err(PaymentServiceError::Payment(PurlError::InvalidChallenge(
                    "Dry-run mode enabled - payment not executed".to_string(),
                )));
            }

            // Create payment credential
            let credential = handler.create_credential(&challenge).await?;

            // Format authorization header
            let auth_header = handler.format_authorization(&credential)?;

            // Retry with payment credential
            let mut retry_request = cloned_request.ok_or_else(|| {
                PaymentServiceError::Payment(PurlError::Http(
                    "Failed to clone request for retry".to_string(),
                ))
            })?;

            retry_request.headers_mut().insert(
                AUTHORIZATION_HEADER,
                auth_header.parse().map_err(|_| {
                    PaymentServiceError::Payment(PurlError::Http(
                        "Invalid authorization header value".to_string(),
                    ))
                })?,
            );

            inner
                .call(retry_request)
                .await
                .map_err(PaymentServiceError::Inner)
        })
    }
}

/// Clone an HTTP request.
///
/// This attempts to clone the request for retry purposes. Returns None if
/// the body cannot be cloned.
fn clone_request<B: Clone>(request: &Request<B>) -> Option<Request<B>> {
    let mut builder = Request::builder()
        .method(request.method().clone())
        .uri(request.uri().clone())
        .version(request.version());

    // Copy headers
    if let Some(headers) = builder.headers_mut() {
        headers.extend(
            request
                .headers()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }

    builder.body(request.body().clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_layer_new() {
        let config = Config::default();
        let _layer = PaymentLayer::from_config(config);
    }

    #[test]
    fn test_payment_layer_builder() {
        let config = Config::default();
        let layer = PaymentLayer::from_config(config)
            .max_amount(1_000_000u128)
            .allowed_networks(&["base"])
            .dry_run(true);

        assert!(layer.handler.is_dry_run());
    }

    #[test]
    fn test_payment_service_error_display() {
        let inner_err: PaymentServiceError<std::io::Error> =
            PaymentServiceError::Inner(std::io::Error::new(std::io::ErrorKind::Other, "test"));
        assert!(inner_err.to_string().contains("inner service error"));

        let payment_err: PaymentServiceError<std::io::Error> =
            PaymentServiceError::Payment(PurlError::Http("test".to_string()));
        assert!(payment_err.to_string().contains("payment error"));
    }
}
