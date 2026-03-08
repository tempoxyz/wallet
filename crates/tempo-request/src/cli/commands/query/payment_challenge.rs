//! Parsing and validation of 402 payment challenges.

use anyhow::{Context as _, Result};
use mpp::protocol::methods::tempo::session::TempoSessionExt;
use mpp::protocol::methods::tempo::TempoChargeExt;

use tempo_common::error::PaymentError;
use tempo_common::fmt::format_token_amount;
use tempo_common::http::HttpResponse;
use tempo_common::network::NetworkId;

/// Parsed payment challenge extracted from a 402 response.
pub(super) struct ParsedChallenge {
    pub(super) is_session: bool,
    pub(super) network: NetworkId,
    pub(super) amount: String,
    pub(super) currency: String,
    pub(super) challenge: mpp::PaymentChallenge,
}

impl ParsedChallenge {
    pub(super) fn intent_str(&self) -> &'static str {
        if self.is_session {
            "session"
        } else {
            "charge"
        }
    }

    /// Format the payment amount for human display, falling back to raw value + symbol.
    pub(super) fn amount_display(&self) -> String {
        self.amount
            .parse::<u128>()
            .ok()
            .map(|a| format_token_amount(a, self.network))
            .unwrap_or_else(|| format!("{} {}", self.amount, self.network.token().symbol))
    }
}

/// Parse the WWW-Authenticate header from a 402 response and extract all
/// payment-related context needed for routing and analytics.
pub(super) fn parse_payment_challenge(response: &HttpResponse) -> Result<ParsedChallenge> {
    let www_auth = response
        .header("www-authenticate")
        .ok_or_else(|| PaymentError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        mpp::parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    // Enforce supported payment protocol (tempo only for now)
    if !challenge.method.eq_ignore_ascii_case("tempo") {
        return Err(PaymentError::UnsupportedPaymentMethod(challenge.method.to_string()).into());
    }

    let is_session = challenge.intent.is_session();

    let (network, amount, currency) =
        if let Ok(charge) = challenge.request.decode::<mpp::ChargeRequest>() {
            (
                require_chain(charge.chain_id())?,
                charge.amount,
                charge.currency,
            )
        } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
            (
                require_chain(session.chain_id())?,
                session.amount,
                session.currency,
            )
        } else {
            return Err(PaymentError::InvalidChallenge(
                "unsupported payment challenge payload".to_string(),
            )
            .into());
        };

    Ok(ParsedChallenge {
        is_session,
        network,
        amount,
        currency,
        challenge,
    })
}

/// Resolve a chain ID to a known `NetworkId`, or fail with a clear error.
fn require_chain(chain_id: Option<u64>) -> Result<NetworkId> {
    let cid = chain_id.ok_or_else(|| {
        PaymentError::InvalidChallenge("missing chainId in payment request".to_string())
    })?;
    NetworkId::from_chain_id(cid)
        .ok_or_else(|| PaymentError::InvalidChallenge(format!("unsupported chainId: {cid}")).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_challenge(intent: &str) -> mpp::PaymentChallenge {
        let request = mpp::Base64UrlJson::from_value(
            &serde_json::json!({"amount": "1000", "currency": "USDC"}),
        )
        .unwrap();
        mpp::PaymentChallenge::new("test-id", "test-realm", "tempo", intent, request)
    }

    fn make_ctx(is_session: bool, amount: &str) -> ParsedChallenge {
        let intent = if is_session { "session" } else { "charge" };
        ParsedChallenge {
            is_session,
            network: NetworkId::default(),
            amount: amount.to_string(),
            currency: "USDC".to_string(),
            challenge: make_challenge(intent),
        }
    }

    #[test]
    fn test_intent_str_session() {
        let ctx = make_ctx(true, "1000");
        assert_eq!(ctx.intent_str(), "session");
    }

    #[test]
    fn test_intent_str_charge() {
        let ctx = make_ctx(false, "1000");
        assert_eq!(ctx.intent_str(), "charge");
    }

    #[test]
    fn test_amount_display_valid_numeric() {
        let ctx = make_ctx(false, "1000000");
        let display = ctx.amount_display();
        assert!(!display.is_empty());
    }

    #[test]
    fn test_amount_display_non_numeric_fallback() {
        let ctx = make_ctx(false, "not-a-number");
        let display = ctx.amount_display();
        assert!(display.contains("not-a-number"));
    }

    #[test]
    fn test_require_chain_known_network() {
        let result = require_chain(Some(4217));
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_chain_unknown_network() {
        let result = require_chain(Some(99999));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unsupported chainId"));
    }

    #[test]
    fn test_require_chain_missing() {
        let result = require_chain(None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing chainId"));
    }

    #[test]
    fn test_parse_payment_challenge_missing_header() {
        let response = HttpResponse::for_test(402, b"");
        let result = parse_payment_challenge(&response);
        let err = result.err().expect("should be an error");
        assert!(err.to_string().contains("WWW-Authenticate"));
    }
}
