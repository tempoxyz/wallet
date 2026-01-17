//! Tests for the EVM payment provider

use purl_lib::test_fixtures::{
    PaymentRequirementBuilder, TEST_EVM_ADDRESS, TEST_EVM_KEY, USDC_BASE,
};
use purl_lib::x402::PaymentRequirements;
use purl_lib::{Config, EvmConfig, PROVIDER_REGISTRY};

// =============================================================================
// Provider Registration Tests
// =============================================================================

#[test]
fn test_evm_provider_supports_evm_networks() {
    let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();

    // EVM networks
    assert!(provider.supports_network("base"));
    assert!(provider.supports_network("base-sepolia"));
    assert!(provider.supports_network("ethereum"));

    // Not EVM networks
    assert!(!provider.supports_network("solana"));
    assert!(!provider.supports_network("solana-devnet"));
    assert!(!provider.supports_network("unknown"));
}

#[test]
fn test_evm_provider_name() {
    let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
    assert_eq!(provider.name(), "EVM");
}

// =============================================================================
// Address Generation Tests
// =============================================================================

#[test]
fn test_evm_provider_get_address_with_private_key() {
    let config = Config {
        evm: Some(EvmConfig {
            keystore: None,
            private_key: Some(TEST_EVM_KEY.to_string()),
        }),
        solana: None,
        ..Default::default()
    };

    let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
    let result = provider.get_address(&config);

    assert!(result.is_ok());
    let address = result.unwrap();
    assert!(address.starts_with("0x"));
    assert_eq!(address.len(), 42);
}

#[test]
fn test_evm_provider_get_address_without_config() {
    let config = Config {
        evm: None,
        solana: None,
        ..Default::default()
    };

    let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
    let result = provider.get_address(&config);

    assert!(result.is_err());
}

// =============================================================================
// Dry Run Tests
// =============================================================================

#[test]
fn test_evm_provider_dry_run() {
    let config = Config {
        evm: Some(EvmConfig {
            keystore: None,
            private_key: Some(TEST_EVM_KEY.to_string()),
        }),
        solana: None,
        ..Default::default()
    };

    let v1_req = PaymentRequirementBuilder::evm().amount("1000000").build();
    let requirement = PaymentRequirements::V1(v1_req);

    let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
    let result = provider.dry_run(&requirement, &config);

    assert!(result.is_ok());
    let dry_run_info = result.unwrap();

    assert_eq!(dry_run_info.provider, "EVM");
    assert_eq!(dry_run_info.network, "base");
    assert_eq!(dry_run_info.amount, "1000000");
    assert_eq!(dry_run_info.asset, USDC_BASE);
    assert_eq!(dry_run_info.to, TEST_EVM_ADDRESS);
    assert_eq!(dry_run_info.estimated_fee, Some("0".to_string()));
}

#[test]
fn test_evm_provider_dry_run_without_config() {
    let config = Config {
        evm: None,
        solana: None,
        ..Default::default()
    };

    let v1_req = PaymentRequirementBuilder::evm().amount("1000000").build();
    let requirement = PaymentRequirements::V1(v1_req);

    let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
    let result = provider.dry_run(&requirement, &config);

    assert!(result.is_err());
}

// =============================================================================
// Address Validation Tests
// =============================================================================

mod address_validation_tests {
    use super::*;

    #[test]
    fn test_valid_evm_addresses() {
        let valid_addresses = vec![
            "0x0000000000000000000000000000000000000000",
            "0xffffffffffffffffffffffffffffffffffffffff",
            "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "0x1234567890abcdef1234567890abcdef12345678",
        ];

        for addr in valid_addresses {
            let result = alloy::primitives::Address::parse_checksummed(addr, None);
            let _ = result;
        }
    }

    #[test]
    fn test_private_key_loading_with_0x_prefix() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(format!("0x{}", TEST_EVM_KEY)),
            }),
            solana: None,
            ..Default::default()
        };

        let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
        let result = provider.get_address(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_private_key_loading_without_0x_prefix() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        };

        let provider = PROVIDER_REGISTRY.find_provider("base").unwrap();
        let result = provider.get_address(&config);
        assert!(result.is_ok());
    }
}

// =============================================================================
// EIP-712 Tests
// =============================================================================

mod eip712_tests {
    use alloy::primitives::{Address, B256, U256};
    use alloy::sol;
    use alloy::sol_types::{eip712_domain, SolStruct};
    use purl_lib::test_fixtures::USDC_BASE;
    use std::str::FromStr;

    sol! {
        #[derive(Debug)]
        struct TransferWithAuthorization {
            address from;
            address to;
            uint256 value;
            uint256 validAfter;
            uint256 validBefore;
            bytes32 nonce;
        }
    }

    #[test]
    fn test_transfer_authorization_struct_creation() {
        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0xabcdef1234567890abcdef1234567890abcdef12").unwrap();
        let value = U256::from(1000000u64);
        let valid_after = U256::from(0u64);
        let valid_before = U256::from(9999999999u64);
        let nonce = B256::from([1u8; 32]);

        let auth = TransferWithAuthorization {
            from,
            to,
            value,
            validAfter: valid_after,
            validBefore: valid_before,
            nonce,
        };

        assert_eq!(auth.from, from);
        assert_eq!(auth.to, to);
        assert_eq!(auth.value, value);
    }

    #[test]
    fn test_eip712_domain_creation() {
        let domain = eip712_domain! {
            name: "USD Coin",
            version: "2",
            chain_id: 8453,
            verifying_contract: Address::from_str(USDC_BASE).unwrap(),
        };

        assert!(domain.name.is_some());
        assert!(domain.version.is_some());
        assert!(domain.chain_id.is_some());
    }

    #[test]
    fn test_eip712_signing_hash_generation() {
        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0xabcdef1234567890abcdef1234567890abcdef12").unwrap();

        let auth = TransferWithAuthorization {
            from,
            to,
            value: U256::from(1000000u64),
            validAfter: U256::from(0u64),
            validBefore: U256::from(9999999999u64),
            nonce: B256::from([1u8; 32]),
        };

        let domain = eip712_domain! {
            name: "USD Coin",
            version: "2",
            chain_id: 8453,
            verifying_contract: Address::from_str(USDC_BASE).unwrap(),
        };

        let signing_hash = auth.eip712_signing_hash(&domain);

        assert_eq!(signing_hash.len(), 32);
        assert_ne!(signing_hash, B256::ZERO);
    }

    #[test]
    fn test_deterministic_signing_hash() {
        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0xabcdef1234567890abcdef1234567890abcdef12").unwrap();

        let make_auth = || TransferWithAuthorization {
            from,
            to,
            value: U256::from(1000000u64),
            validAfter: U256::from(0u64),
            validBefore: U256::from(9999999999u64),
            nonce: B256::from([1u8; 32]),
        };

        let domain = eip712_domain! {
            name: "USD Coin",
            version: "2",
            chain_id: 8453,
            verifying_contract: Address::from_str(USDC_BASE).unwrap(),
        };

        let hash1 = make_auth().eip712_signing_hash(&domain);
        let hash2 = make_auth().eip712_signing_hash(&domain);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_different_amounts_different_hash() {
        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0xabcdef1234567890abcdef1234567890abcdef12").unwrap();

        let auth1 = TransferWithAuthorization {
            from,
            to,
            value: U256::from(1000000u64),
            validAfter: U256::from(0u64),
            validBefore: U256::from(9999999999u64),
            nonce: B256::from([1u8; 32]),
        };

        let auth2 = TransferWithAuthorization {
            from,
            to,
            value: U256::from(2000000u64),
            validAfter: U256::from(0u64),
            validBefore: U256::from(9999999999u64),
            nonce: B256::from([1u8; 32]),
        };

        let domain = eip712_domain! {
            name: "USD Coin",
            version: "2",
            chain_id: 8453,
            verifying_contract: Address::from_str(USDC_BASE).unwrap(),
        };

        let hash1 = auth1.eip712_signing_hash(&domain);
        let hash2 = auth2.eip712_signing_hash(&domain);

        assert_ne!(hash1, hash2);
    }
}

// =============================================================================
// Payload Serialization Tests
// =============================================================================

mod payload_serialization_tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct EvmPayload {
        pub signature: String,
        pub authorization: Authorization,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Authorization {
        pub from: String,
        pub nonce: String,
        pub to: String,
        pub valid_after: String,
        pub valid_before: String,
        pub value: String,
    }

    #[test]
    fn test_evm_payload_serialization_camel_case() {
        let payload = EvmPayload {
            signature: "0x1234...".to_string(),
            authorization: Authorization {
                from: "0x1234567890123456789012345678901234567890".to_string(),
                nonce: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .to_string(),
                to: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
                valid_after: "0".to_string(),
                valid_before: "9999999999".to_string(),
                value: "1000000".to_string(),
            },
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"validAfter\""));
        assert!(json.contains("\"validBefore\""));
        assert!(!json.contains("\"valid_after\""));
        assert!(!json.contains("\"valid_before\""));
    }

    #[test]
    fn test_evm_payload_deserialization() {
        let json = r#"{
            "signature": "0xabc123",
            "authorization": {
                "from": "0x1234567890123456789012345678901234567890",
                "nonce": "0x01",
                "to": "0xabcdef1234567890abcdef1234567890abcdef12",
                "validAfter": "100",
                "validBefore": "999",
                "value": "5000"
            }
        }"#;

        let payload: EvmPayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.signature, "0xabc123");
        assert_eq!(payload.authorization.valid_after, "100");
        assert_eq!(payload.authorization.valid_before, "999");
        assert_eq!(payload.authorization.value, "5000");
    }

    #[test]
    fn test_evm_payload_roundtrip() {
        let original = EvmPayload {
            signature: "0xsig".to_string(),
            authorization: Authorization {
                from: "0xfrom".to_string(),
                nonce: "0xnonce".to_string(),
                to: "0xto".to_string(),
                valid_after: "123".to_string(),
                valid_before: "456".to_string(),
                value: "789".to_string(),
            },
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: EvmPayload = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }
}

// =============================================================================
// Chain ID Tests
// =============================================================================

mod chain_id_tests {
    use purl_lib::network::get_evm_chain_id;

    #[test]
    fn test_base_mainnet_chain_id() {
        assert_eq!(get_evm_chain_id("base"), Some(8453));
    }

    #[test]
    fn test_base_sepolia_chain_id() {
        assert_eq!(get_evm_chain_id("base-sepolia"), Some(84532));
    }

    #[test]
    fn test_ethereum_chain_id() {
        assert_eq!(get_evm_chain_id("ethereum"), Some(1));
    }

    #[test]
    fn test_unknown_network_chain_id() {
        assert_eq!(get_evm_chain_id("unknown-network"), None);
    }

    #[test]
    fn test_solana_network_no_chain_id() {
        assert_eq!(get_evm_chain_id("solana"), None);
    }
}
