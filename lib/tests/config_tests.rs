//! Integration tests for configuration management

use purl::{Config, EvmConfig, PaymentMethod};

#[test]
fn test_config_serialization_roundtrip() {
    let config = Config {
        evm: Some(EvmConfig {
            keystore: None,
            private_key: Some(
                "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            ),
        }),
        ..Default::default()
    };

    let toml_str = toml::to_string_pretty(&config).expect("Failed to serialize");
    let deserialized: Config = toml::from_str(&toml_str).expect("Failed to deserialize");

    assert!(deserialized.evm.is_some());
    assert_eq!(
        deserialized.evm.as_ref().unwrap().private_key,
        config.evm.as_ref().unwrap().private_key
    );
}

#[test]
fn test_available_payment_methods() {
    struct TestCase {
        evm: Option<EvmConfig>,
        expected_len: usize,
        should_contain_evm: bool,
    }

    let test_cases = vec![
        TestCase {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some("test".to_string()),
            }),
            expected_len: 1,
            should_contain_evm: true,
        },
        TestCase {
            evm: None,
            expected_len: 0,
            should_contain_evm: false,
        },
    ];

    for test_case in test_cases {
        let config = Config {
            evm: test_case.evm,
            ..Default::default()
        };

        let methods = config.available_payment_methods();
        assert_eq!(
            methods.len(),
            test_case.expected_len,
            "Expected {} payment methods",
            test_case.expected_len
        );
        assert_eq!(
            methods.contains(&PaymentMethod::Evm),
            test_case.should_contain_evm,
            "EVM method presence should be {}",
            test_case.should_contain_evm
        );
    }
}

#[test]
fn test_config_validation_evm() {
    struct TestCase {
        private_key: &'static str,
        should_be_valid: bool,
        description: &'static str,
    }

    let test_cases = vec![
        TestCase {
            private_key: "tooshort",
            should_be_valid: false,
            description: "short key",
        },
        TestCase {
            private_key: "bad",
            should_be_valid: false,
            description: "non-hex key",
        },
        TestCase {
            private_key: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            should_be_valid: true,
            description: "valid key without 0x prefix",
        },
        TestCase {
            private_key: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            should_be_valid: true,
            description: "valid key with 0x prefix",
        },
    ];

    for test_case in test_cases {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(test_case.private_key.to_string()),
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert_eq!(
            result.is_ok(),
            test_case.should_be_valid,
            "EVM config with {} should {}",
            test_case.description,
            if test_case.should_be_valid {
                "be valid"
            } else {
                "be invalid"
            }
        );

        if !test_case.should_be_valid && test_case.description == "short key" {
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("private key") || err_msg.contains("64"),
                "Error message should mention private key or 64"
            );
        }
    }
}

#[test]
fn test_payment_method_as_str() {
    let test_cases = vec![(PaymentMethod::Evm, "evm")];

    for (method, expected_str) in test_cases {
        assert_eq!(
            method.as_str(),
            expected_str,
            "PaymentMethod::{method:?} should have string representation '{expected_str}'"
        );
    }
}

#[test]
fn test_config_partial_deserialization() {
    let toml = r#"
        [evm]
        private_key = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    "#;

    let config: Config = toml::from_str(toml).expect("Failed to parse");
    assert!(config.evm.is_some());
}

#[test]
fn test_config_empty_is_valid() {
    let toml = r#""#;

    let config: Config = toml::from_str(toml).expect("Failed to parse empty config");
    assert!(config.evm.is_none());
}
