//! Tests for Web Payment Auth protocol handling
//!
//! These tests verify the library-level web payment types and validation.

use purl_lib::protocol::web::{
    ChargeRequest, PaymentChallenge, PaymentIntent, PaymentMethod, PaymentProtocol,
};

#[test]
fn test_protocol_detection_web_payment() {
    assert_eq!(
        PaymentProtocol::detect(Some("Payment id=\"abc\"")),
        PaymentProtocol::WebPaymentAuth
    );
}

#[test]
fn test_protocol_detection_x402_fallback() {
    assert_eq!(PaymentProtocol::detect(None), PaymentProtocol::X402);
    assert_eq!(
        PaymentProtocol::detect(Some("Bearer token")),
        PaymentProtocol::X402
    );
}

#[test]
fn test_charge_request_parse_amount_valid() {
    let req = ChargeRequest {
        amount: "1000000".to_string(),
        asset: "0x123".to_string(),
        destination: "0x456".to_string(),
        expires: "2024-01-01T00:00:00Z".to_string(),
        fee_payer: None,
    };

    assert_eq!(req.parse_amount().unwrap(), 1_000_000u128);
}

#[test]
fn test_charge_request_parse_amount_invalid() {
    let req = ChargeRequest {
        amount: "not-a-number".to_string(),
        asset: "0x123".to_string(),
        destination: "0x456".to_string(),
        expires: "2024-01-01T00:00:00Z".to_string(),
        fee_payer: None,
    };

    assert!(req.parse_amount().is_err());
}

#[test]
fn test_charge_request_validate_max_amount_ok() {
    let req = ChargeRequest {
        amount: "500000".to_string(),
        asset: "0x123".to_string(),
        destination: "0x456".to_string(),
        expires: "2024-01-01T00:00:00Z".to_string(),
        fee_payer: None,
    };

    assert!(req.validate_max_amount("1000000").is_ok());
}

#[test]
fn test_charge_request_validate_max_amount_exceeds() {
    let req = ChargeRequest {
        amount: "2000000".to_string(),
        asset: "0x123".to_string(),
        destination: "0x456".to_string(),
        expires: "2024-01-01T00:00:00Z".to_string(),
        fee_payer: None,
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
        asset: "0x123".to_string(),
        destination: "0x456".to_string(),
        expires: "2024-01-01T00:00:00Z".to_string(),
        fee_payer: None,
    };

    assert!(req.validate_max_amount("not-a-number").is_err());
}

#[test]
fn test_payment_challenge_validate_tempo_charge() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Tempo,
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert!(challenge.validate().is_ok());
}

#[test]
fn test_payment_challenge_validate_base_charge() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Base,
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert!(challenge.validate().is_ok());
}

#[test]
fn test_payment_challenge_validate_unsupported_method() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Custom("unknown".to_string()),
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert!(challenge.validate().is_err());
}

#[test]
fn test_payment_challenge_validate_unsupported_intent() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Tempo,
        intent: PaymentIntent::Authorize, // Not supported yet
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert!(challenge.validate().is_err());
}

#[test]
fn test_payment_challenge_network_name_tempo() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Tempo,
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert_eq!(challenge.network_name().unwrap(), "tempo-moderato");
}

#[test]
fn test_payment_challenge_network_name_base() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Base,
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert_eq!(challenge.network_name().unwrap(), "base-sepolia");
}

#[test]
fn test_payment_challenge_network_name_custom_fails() {
    let challenge = PaymentChallenge {
        id: "test123".to_string(),
        realm: "api".to_string(),
        method: PaymentMethod::Custom("unknown".to_string()),
        intent: PaymentIntent::Charge,
        request: serde_json::json!({}),
        expires: None,
        description: None,
    };

    assert!(challenge.network_name().is_err());
}
