//! Tests for Web Payment Auth protocol handling
//!
//! These tests verify the library-level web payment types and validation.

use purl::protocol::web::{
    ChargeRequest, PaymentChallenge, PaymentIntent, PaymentMethod, PaymentProtocol,
};

fn test_charge_request() -> ChargeRequest {
    ChargeRequest {
        amount: "1000000".to_string(),
        currency: "0x123".to_string(),
        recipient: Some("0x456".to_string()),
        expires: Some("2024-01-01T00:00:00Z".to_string()),
        description: None,
        external_id: None,
        method_details: None,
    }
}

fn test_challenge() -> PaymentChallenge {
    PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Tempo,
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        request_raw: String::new(),
        digest: None,
        expires: None,
        description: None,
    }
}

#[test]
fn test_protocol_detection_web_payment() {
    assert_eq!(
        PaymentProtocol::detect(Some("Payment id=\"abc\"")),
        Some(PaymentProtocol::WebPaymentAuth)
    );
}

#[test]
fn test_charge_request_parse_amount_valid() {
    let req = test_charge_request();
    assert_eq!(req.parse_amount().unwrap(), 1_000_000u128);
}

#[test]
fn test_charge_request_parse_amount_invalid() {
    let req = ChargeRequest {
        amount: "not-a-number".to_string(),
        ..test_charge_request()
    };
    assert!(req.parse_amount().is_err());
}

#[test]
fn test_charge_request_validate_max_amount_ok() {
    let req = ChargeRequest {
        amount: "500000".to_string(),
        ..test_charge_request()
    };
    assert!(req.validate_max_amount("1000000").is_ok());
}

#[test]
fn test_charge_request_validate_max_amount_exceeds() {
    let req = ChargeRequest {
        amount: "2000000".to_string(),
        ..test_charge_request()
    };
    let result = req.validate_max_amount("1000000");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("exceeds"));
}

#[test]
fn test_charge_request_validate_max_amount_invalid_max() {
    let req = ChargeRequest {
        amount: "500000".to_string(),
        ..test_charge_request()
    };
    assert!(req.validate_max_amount("not-a-number").is_err());
}

#[test]
fn test_payment_challenge_validate_tempo_charge() {
    let challenge = test_challenge();
    assert!(challenge.validate().is_ok());
}

#[test]
fn test_payment_challenge_validate_unsupported_method() {
    let challenge = PaymentChallenge {
        method: PaymentMethod::Custom("unknown".to_string()),
        ..test_challenge()
    };
    assert!(challenge.validate().is_err());
}

#[test]
fn test_payment_challenge_validate_unsupported_intent() {
    let challenge = PaymentChallenge {
        intent: PaymentIntent::Authorize, // Not supported yet
        ..test_challenge()
    };
    assert!(challenge.validate().is_err());
}

#[test]
fn test_payment_challenge_network_name_tempo() {
    let challenge = test_challenge();
    assert_eq!(challenge.network_name().unwrap(), "tempo-moderato");
}

#[test]
fn test_payment_challenge_network_name_custom_fails() {
    let challenge = PaymentChallenge {
        method: PaymentMethod::Custom("unknown".to_string()),
        ..test_challenge()
    };
    assert!(challenge.network_name().is_err());
}
