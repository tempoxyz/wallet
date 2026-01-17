//! Tests for the PurlClient public API
//!
//! These tests cover the public builder pattern and configuration options.
//! Private implementation details (like detect_protocol) are tested inline in client.rs.

use purl_lib::test_fixtures::{create_200_response, create_test_response, test_config_evm_only};
use purl_lib::{Config, PurlClient};
use std::collections::HashMap;

// =============================================================================
// Builder Pattern Tests
// =============================================================================

#[test]
fn test_purl_client_with_config_defaults() {
    let config = Config::default();
    let _client = PurlClient::with_config(config);
}

#[test]
fn test_purl_client_builder_chaining() {
    let config = test_config_evm_only();

    let _client = PurlClient::with_config(config)
        .max_amount("1000000")
        .allowed_networks(&["base", "ethereum"])
        .header("X-Custom-Header", "value")
        .timeout(30)
        .follow_redirects()
        .user_agent("test-agent/1.0")
        .verbose()
        .dry_run();
}

#[test]
fn test_purl_client_max_amount_str() {
    let config = Config::default();
    let _client = PurlClient::with_config(config).max_amount("1000000");
}

#[test]
fn test_purl_client_max_amount_string() {
    let config = Config::default();
    let _client = PurlClient::with_config(config).max_amount(String::from("999999"));
}

#[test]
fn test_purl_client_max_amount_large() {
    let config = Config::default();
    let _client = PurlClient::with_config(config).max_amount("999999999999999999999999999999");
}

#[test]
fn test_purl_client_allowed_networks_empty() {
    let config = Config::default();
    let _client = PurlClient::with_config(config).allowed_networks(&[]);
}

#[test]
fn test_purl_client_allowed_networks_multiple() {
    let config = Config::default();
    let _client = PurlClient::with_config(config).allowed_networks(&[
        "base",
        "ethereum",
        "solana",
        "base-sepolia",
    ]);
}

#[test]
fn test_purl_client_multiple_headers() {
    let config = Config::default();
    let _client = PurlClient::with_config(config)
        .header("Authorization", "Bearer token123")
        .header("X-API-Key", "key456")
        .header("Content-Type", "application/json");
}

#[test]
fn test_purl_client_new_without_config_file() {
    let result = PurlClient::new();
    let _ = result;
}

// =============================================================================
// HttpResponse Helper Tests
// =============================================================================

mod http_response_tests {
    use super::*;

    #[test]
    fn test_is_payment_required_402() {
        let response = create_test_response(402, HashMap::new(), b"");
        assert!(response.is_payment_required());
    }

    #[test]
    fn test_is_payment_required_other_codes() {
        for code in [200, 201, 301, 400, 401, 403, 404, 500, 502, 503] {
            let response = create_test_response(code, HashMap::new(), b"");
            assert!(
                !response.is_payment_required(),
                "Status {} should not be payment required",
                code
            );
        }
    }

    #[test]
    fn test_get_header_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let response = create_test_response(200, headers, b"");

        assert_eq!(
            response.get_header("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            response.get_header("Content-Type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            response.get_header("CONTENT-TYPE"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_get_header_missing() {
        let response = create_200_response(b"");
        assert_eq!(response.get_header("non-existent"), None);
    }

    #[test]
    fn test_body_string_valid_utf8() {
        let response = create_200_response(b"Hello, World!");
        assert_eq!(response.body_string().unwrap(), "Hello, World!");
    }

    #[test]
    fn test_body_string_empty() {
        let response = create_200_response(b"");
        assert_eq!(response.body_string().unwrap(), "");
    }

    #[test]
    fn test_body_string_invalid_utf8() {
        let response = create_test_response(200, HashMap::new(), &[0xff, 0xfe, 0x00, 0x01]);
        let result = response.body_string();
        assert!(result.is_err(), "Invalid UTF-8 should return error");
    }

    #[test]
    fn test_body_string_json() {
        let json_body = br#"{"key": "value", "number": 42}"#;
        let response = create_200_response(json_body);
        let body = response.body_string().unwrap();
        assert!(body.contains("key"));
        assert!(body.contains("value"));
        assert!(body.contains("42"));
    }
}

// =============================================================================
// Protocol Header Detection Tests
// =============================================================================

mod protocol_detection_tests {
    use purl_lib::test_fixtures::create_response_with_headers;

    #[test]
    fn test_x402_v1_header_presence() {
        let response = create_response_with_headers(
            402,
            vec![("x-payment-response", "some-payment-data")],
            b"{}",
        );

        assert!(response.is_payment_required());
        assert!(response.get_header("x-payment-response").is_some());
    }

    #[test]
    fn test_x402_v2_header_presence() {
        let response = create_response_with_headers(
            402,
            vec![("payment-required", "eyJ2ZXJzaW9uIjogIjIifQ==")],
            b"{}",
        );

        assert!(response.is_payment_required());
        assert!(response.get_header("payment-required").is_some());
    }

    #[test]
    fn test_web_payment_auth_header() {
        let response = create_response_with_headers(
            402,
            vec![(
                "www-authenticate",
                r#"Payment id="abc", realm="api", method="tempo", intent="charge", request="eyJ9""#,
            )],
            b"",
        );

        assert!(response.is_payment_required());
        let www_auth = response.get_header("www-authenticate").unwrap();
        assert!(www_auth.starts_with("Payment "));
    }
}

// =============================================================================
// PaymentResult Enum Tests
// =============================================================================

mod payment_result_tests {
    use purl_lib::client::PaymentResult;
    use purl_lib::payment_provider::DryRunInfo;
    use purl_lib::test_fixtures::{create_200_response, TEST_EVM_ADDRESS, USDC_BASE};

    #[test]
    fn test_payment_result_success_variant() {
        let response = create_200_response(b"success");
        let result = PaymentResult::Success(response);

        match result {
            PaymentResult::Success(r) => {
                assert_eq!(r.status_code, 200);
                assert_eq!(r.body_string().unwrap(), "success");
            }
            _ => panic!("Expected Success variant"),
        }
    }

    #[test]
    fn test_payment_result_dry_run_variant() {
        let dry_run_info = DryRunInfo {
            provider: "EVM".to_string(),
            network: "base".to_string(),
            amount: "1000000".to_string(),
            asset: USDC_BASE.to_string(),
            from: TEST_EVM_ADDRESS.to_string(),
            to: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
            estimated_fee: Some("0".to_string()),
        };

        let result = PaymentResult::DryRun(dry_run_info);

        match result {
            PaymentResult::DryRun(info) => {
                assert_eq!(info.provider, "EVM");
                assert_eq!(info.network, "base");
                assert_eq!(info.amount, "1000000");
                assert_eq!(info.asset, USDC_BASE);
            }
            _ => panic!("Expected DryRun variant"),
        }
    }

    #[test]
    fn test_payment_result_debug_format() {
        let response = create_200_response(b"test");
        let result = PaymentResult::Success(response);

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Success"));
    }
}

// =============================================================================
// Property-Based Tests (using proptest)
// =============================================================================

#[cfg(test)]
mod proptest_tests {
    use proptest::prelude::*;
    use purl_lib::{Config, PurlClient};

    proptest! {
        #[test]
        fn test_max_amount_accepts_any_numeric_string(amount in "[0-9]{1,30}") {
            let config = Config::default();
            let _client = PurlClient::with_config(config).max_amount(&amount);
        }

        #[test]
        fn test_timeout_any_u64(timeout in 0u64..=u64::MAX) {
            let config = Config::default();
            let _client = PurlClient::with_config(config).timeout(timeout);
        }

        #[test]
        fn test_user_agent_any_string(ua in ".*") {
            let config = Config::default();
            let _client = PurlClient::with_config(config).user_agent(&ua);
        }

        #[test]
        fn test_network_names_any_string(network in "[a-z0-9-]{1,50}") {
            let config = Config::default();
            let networks: Vec<&str> = vec![&network];
            let _client = PurlClient::with_config(config).allowed_networks(&networks);
        }
    }
}
