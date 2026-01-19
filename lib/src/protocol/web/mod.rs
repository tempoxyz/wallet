//! Web Payment Auth Protocol (IETF draft-ietf-httpauth-payment-01)
//!
//! This module implements support for the IETF Payment Auth protocol, which uses
//! standard HTTP authentication headers for blockchain-based micropayments.
//!
//! # Protocol Overview
//!
//! 1. Client requests a protected resource
//! 2. Server responds with 402 Payment Required and `WWW-Authenticate: Payment` header
//! 3. Client signs a blockchain transaction and sends it in `Authorization: Payment` header
//! 4. Server verifies the transaction, broadcasts it, and returns `Payment-Receipt` header
//!
//! # Example
//!
//! ```no_run
//! # use purl::protocol::web::{PaymentChallenge, PaymentCredential, PaymentMethod, PaymentIntent};
//! # fn main() -> Result<(), purl::error::PurlError> {
//! // Parse challenge from WWW-Authenticate header
//! let header = "Payment id=\"abc123\", realm=\"api\"";
//! let challenge = purl::protocol::web::parse_www_authenticate(header)?;
//!
//! // After creating payment credential via provider...
//! // Format Authorization header for retrying the request
//! # let credential = PaymentCredential {
//! #     id: "abc123".to_string(),
//! #     source: None,
//! #     payload: purl::protocol::web::PaymentPayload {
//! #         payload_type: purl::protocol::web::PayloadType::Transaction,
//! #         signature: "0x...".to_string(),
//! #     },
//! # };
//! let auth_header = purl::protocol::web::format_authorization(&credential)?;
//! # Ok(())
//! # }
//! ```

pub mod encode;
pub mod parse;
pub mod types;

pub use encode::{
    base64url_decode, base64url_encode, format_authorization, format_receipt,
    format_www_authenticate,
};
pub use parse::{parse_authorization, parse_receipt, parse_www_authenticate};
pub use types::{
    AuthorizeRequest, ChargeRequest, PayloadType, PaymentChallenge, PaymentCredential,
    PaymentIntent, PaymentMethod, PaymentPayload, PaymentProtocol, PaymentReceipt, ReceiptStatus,
    SubscriptionRequest,
};

/// Header name for payment challenges (from server)
pub const WWW_AUTHENTICATE_HEADER: &str = "www-authenticate";

/// Header name for payment credentials (from client)
pub const AUTHORIZATION_HEADER: &str = "authorization";

/// Header name for payment receipts (from server)
pub const PAYMENT_RECEIPT_HEADER: &str = "payment-receipt";

/// Scheme identifier for the Payment authentication scheme
pub const PAYMENT_SCHEME: &str = "Payment";
