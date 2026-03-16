//! Parsing and validation of 402 payment challenges.

use mpp::protocol::methods::tempo::TempoChargeExt;

use crate::{
    http::HttpResponse,
    payment::challenge::{decode_session_request, require_session_chain_id},
};
use tempo_common::{
    cli::{format::format_token_amount, terminal::sanitize_for_terminal},
    error::{PaymentError, TempoError},
    network::NetworkId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedPaymentMethod {
    Tempo,
}

impl SupportedPaymentMethod {
    fn parse(value: &str) -> Option<Self> {
        value.eq_ignore_ascii_case("tempo").then_some(Self::Tempo)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedPaymentIntent {
    Session,
    Charge,
}

impl SupportedPaymentIntent {
    fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("session") {
            Some(Self::Session)
        } else if value.eq_ignore_ascii_case("charge") {
            Some(Self::Charge)
        } else {
            None
        }
    }
}

/// Parsed payment challenge extracted from a 402 response.
pub(crate) struct ParsedChallenge {
    pub(crate) is_session: bool,
    pub(crate) network: NetworkId,
    pub(crate) amount: String,
    pub(crate) currency: String,
    pub(crate) challenge: mpp::PaymentChallenge,
}

impl ParsedChallenge {
    pub(crate) const fn intent_str(&self) -> &'static str {
        if self.is_session {
            "session"
        } else {
            "charge"
        }
    }

    /// Format the payment amount for human display, falling back to raw value + symbol.
    pub(crate) fn amount_display(&self) -> String {
        self.amount
            .parse::<u128>()
            .ok()
            .map(|a| format_token_amount(a, self.network))
            .unwrap_or_else(|| {
                sanitize_for_terminal(&format!("{} {}", self.amount, self.network.token().symbol))
            })
    }
}

/// Parse the WWW-Authenticate header from a 402 response and extract all
/// payment-related context needed for routing and analytics.
pub(crate) fn parse_payment_challenge(
    response: &HttpResponse,
) -> Result<ParsedChallenge, TempoError> {
    let www_auth = response
        .header("www-authenticate")
        .ok_or_else(|| PaymentError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge = mpp::parse_www_authenticate(www_auth).map_err(|source| {
        PaymentError::ChallengeParseSource {
            context: "WWW-Authenticate header",
            source: Box::new(source),
        }
    })?;

    // Enforce supported payment protocol (tempo only for now)
    let method = challenge.method.to_string();
    if SupportedPaymentMethod::parse(&method).is_none() {
        return Err(PaymentError::UnsupportedPaymentMethod(challenge.method.to_string()).into());
    }

    let intent = SupportedPaymentIntent::parse(&challenge.intent.to_string())
        .ok_or_else(|| PaymentError::UnsupportedPaymentIntent(challenge.intent.to_string()))?;

    let (is_session, network, amount, currency) = match intent {
        SupportedPaymentIntent::Session => {
            let session = decode_session_request(&challenge)?;
            (
                true,
                require_chain(Some(require_session_chain_id(
                    &session,
                    "session request methodDetails",
                )?))?,
                session.amount,
                session.currency,
            )
        }
        SupportedPaymentIntent::Charge => {
            let charge = challenge
                .request
                .decode::<mpp::ChargeRequest>()
                .map_err(|_| PaymentError::ChallengeUnsupportedPayload {
                    context: "payment challenge payload",
                })?;
            (
                false,
                require_chain(charge.chain_id())?,
                charge.amount,
                charge.currency,
            )
        }
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
fn require_chain(chain_id: Option<u64>) -> Result<NetworkId, TempoError> {
    let cid = chain_id.ok_or(PaymentError::ChallengeMissingField {
        context: "payment request",
        field: "chainId",
    })?;
    NetworkId::from_chain_id(cid).ok_or_else(|| {
        PaymentError::ChallengeUnsupportedChainId {
            context: "payment request",
            chain_id: cid,
        }
        .into()
    })
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

    fn make_challenge_with_payload(
        intent: &str,
        payload: serde_json::Value,
    ) -> mpp::PaymentChallenge {
        let request = mpp::Base64UrlJson::from_value(&payload).unwrap();
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

    #[test]
    fn test_parse_payment_challenge_session_missing_chainid_is_rejected() {
        let request = mpp::Base64UrlJson::from_value(&serde_json::json!({
            "amount": "1000",
            "currency": "0x20c0000000000000000000000000000000000000",
            "recipient": "0x1111111111111111111111111111111111111111",
            "methodDetails": {
                "escrowContract": "0x542831e3e4ace07559b7c8787395f4fb99f70787"
            }
        }))
        .unwrap();
        let challenge = mpp::PaymentChallenge::new("id", "realm", "tempo", "session", request);
        let www_auth = mpp::format_www_authenticate(&challenge).unwrap();

        let response =
            HttpResponse::for_test_with_headers(402, b"", &[("www-authenticate", &www_auth)]);
        let err = match parse_payment_challenge(&response) {
            Ok(_) => panic!("expected session challenge without chainId to fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("missing chainId"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_parse_payment_challenge_unknown_intent_is_rejected_for_charge_payload() {
        let challenge = make_challenge_with_payload(
            "unexpected",
            serde_json::json!({
                "amount": "1000",
                "currency": "0x20c0000000000000000000000000000000000000",
                "payTo": "0x1111111111111111111111111111111111111111",
                "methodDetails": {
                    "chainId": 4217,
                    "token": "0x20c0000000000000000000000000000000000000"
                }
            }),
        );
        let www_auth = mpp::format_www_authenticate(&challenge).unwrap();
        let response =
            HttpResponse::for_test_with_headers(402, b"", &[("www-authenticate", &www_auth)]);

        let err = match parse_payment_challenge(&response) {
            Ok(_) => panic!("expected unsupported intent to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("Unsupported payment intent"));
    }

    #[test]
    fn test_parse_payment_challenge_unknown_intent_is_rejected_for_session_payload() {
        let challenge = make_challenge_with_payload(
            "unexpected",
            serde_json::json!({
                "amount": "1000",
                "currency": "0x20c0000000000000000000000000000000000000",
                "recipient": "0x1111111111111111111111111111111111111111",
                "methodDetails": {
                    "chainId": 4217,
                    "escrowContract": "0x542831e3e4ace07559b7c8787395f4fb99f70787"
                }
            }),
        );
        let www_auth = mpp::format_www_authenticate(&challenge).unwrap();
        let response =
            HttpResponse::for_test_with_headers(402, b"", &[("www-authenticate", &www_auth)]);

        let err = match parse_payment_challenge(&response) {
            Ok(_) => panic!("expected unsupported intent to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("Unsupported payment intent"));
    }
}
