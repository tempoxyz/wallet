//! Integration tests for payment providers

use purl::{Config, EvmConfig, PROVIDER_REGISTRY};

#[test]
fn test_provider_registry_is_initialized() {
    let registry = &*PROVIDER_REGISTRY;
    assert!(registry.find_provider("base").is_some());
}

#[test]
fn test_find_provider_for_networks() {
    let registry = &*PROVIDER_REGISTRY;

    let test_cases = vec![
        ("ethereum", true),
        ("base", true),
        ("base-sepolia", true),
        ("xx-unknown", false),
    ];

    for (network, should_exist) in test_cases {
        assert_eq!(
            registry.find_provider(network).is_some(),
            should_exist,
            "Provider for network {} should {}",
            network,
            if should_exist { "exist" } else { "not exist" }
        );
    }
}

#[test]
fn test_provider_not_found_for_unknown_network() {
    let registry = &*PROVIDER_REGISTRY;

    let test_cases = vec!["unknown-network", "bitcoin", ""];

    for network in test_cases {
        assert!(
            registry.find_provider(network).is_none(),
            "Provider for unknown network '{network}' should not exist"
        );
    }
}

#[test]
fn test_provider_names() {
    let registry = &*PROVIDER_REGISTRY;

    let test_cases = vec![("base", "EVM")];

    for (network, expected_name) in test_cases {
        let provider = registry
            .find_provider(network)
            .unwrap_or_else(|| panic!("Should find {network} provider"));

        assert_eq!(
            provider.name(),
            expected_name,
            "Provider for network {network} should have name {expected_name}"
        );
    }
}

#[test]
fn test_validate_empty_config() {
    let config = Config {
        evm: None,
        ..Default::default()
    };

    // Empty config should be valid (no wallets configured is OK)
    let result = config.validate();
    assert!(result.is_ok());
}

#[test]
fn test_validate_evm_config() {
    let test_cases = vec![
        (
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            true,
            "valid EVM key without 0x prefix",
        ),
        (
            "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            true,
            "valid EVM key with 0x prefix",
        ),
        ("tooshort", false, "short key"),
        ("bad", false, "non-hex key"),
    ];

    for (private_key, should_be_valid, description) in test_cases {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(private_key.to_string()),
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert_eq!(
            result.is_ok(),
            should_be_valid,
            "EVM config with {} should {}",
            description,
            if should_be_valid {
                "be valid"
            } else {
                "be invalid"
            }
        );
    }
}

#[test]
fn test_provider_supports_correct_networks() {
    let registry = &*PROVIDER_REGISTRY;

    let evm_provider = registry.find_provider("base").unwrap();
    let evm_test_cases = vec![("base", true), ("ethereum", true)];

    for (network, should_support) in evm_test_cases {
        assert_eq!(
            evm_provider.supports_network(network),
            should_support,
            "EVM provider should {} support network {}",
            if should_support { "" } else { "not" },
            network
        );
    }
}

#[test]
fn test_find_provider_is_case_sensitive() {
    let registry = &*PROVIDER_REGISTRY;

    let test_cases = vec![("base", true), ("BASE", false), ("Base", false)];

    for (network, should_exist) in test_cases {
        assert_eq!(
            registry.find_provider(network).is_some(),
            should_exist,
            "Provider lookup for '{}' should {} (case-sensitive)",
            network,
            if should_exist { "succeed" } else { "fail" }
        );
    }
}
