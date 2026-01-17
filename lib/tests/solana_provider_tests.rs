//! Tests for the Solana payment provider

use purl_lib::test_fixtures::{
    PaymentRequirementBuilder, SPL_TOKEN_2022_PROGRAM, SPL_TOKEN_PROGRAM, TEST_SOLANA_ADDRESS,
    TEST_SOLANA_KEY, USDC_SOLANA,
};
use purl_lib::x402::PaymentRequirements;
use purl_lib::{Config, SolanaConfig, PROVIDER_REGISTRY};

// =============================================================================
// Provider Registration Tests
// =============================================================================

#[test]
fn test_solana_provider_supports_solana_networks() {
    let provider = PROVIDER_REGISTRY.find_provider("solana").unwrap();

    // Solana networks
    assert!(provider.supports_network("solana"));
    assert!(provider.supports_network("solana-devnet"));

    // Not Solana networks
    assert!(!provider.supports_network("base"));
    assert!(!provider.supports_network("ethereum"));
    assert!(!provider.supports_network("unknown"));
}

#[test]
fn test_solana_provider_name() {
    let provider = PROVIDER_REGISTRY.find_provider("solana").unwrap();
    assert_eq!(provider.name(), "Solana");
}

// =============================================================================
// Address Generation Tests
// =============================================================================

#[test]
fn test_solana_provider_get_address_with_private_key() {
    let config = Config {
        evm: None,
        solana: Some(SolanaConfig {
            keystore: None,
            private_key: Some(TEST_SOLANA_KEY.to_string()),
        }),
        ..Default::default()
    };

    let provider = PROVIDER_REGISTRY.find_provider("solana").unwrap();
    let result = provider.get_address(&config);

    assert!(result.is_ok());
    let address = result.unwrap();
    assert!(!address.is_empty());
    assert!(address.len() >= 32 && address.len() <= 44);
}

#[test]
fn test_solana_provider_get_address_without_config() {
    let config = Config {
        evm: None,
        solana: None,
        ..Default::default()
    };

    let provider = PROVIDER_REGISTRY.find_provider("solana").unwrap();
    let result = provider.get_address(&config);

    assert!(result.is_err());
}

// =============================================================================
// Dry Run Tests
// =============================================================================

#[test]
fn test_solana_provider_dry_run() {
    let config = Config {
        evm: None,
        solana: Some(SolanaConfig {
            keystore: None,
            private_key: Some(TEST_SOLANA_KEY.to_string()),
        }),
        ..Default::default()
    };

    let v1_req = PaymentRequirementBuilder::solana()
        .amount("1000000")
        .build();
    let requirement = PaymentRequirements::V1(v1_req);

    let provider = PROVIDER_REGISTRY.find_provider("solana").unwrap();
    let result = provider.dry_run(&requirement, &config);

    assert!(result.is_ok());
    let dry_run_info = result.unwrap();

    assert!(dry_run_info.provider.contains("Solana"));
    assert_eq!(dry_run_info.network, "solana");
    assert_eq!(dry_run_info.amount, "1000000");
    assert_eq!(dry_run_info.asset, USDC_SOLANA);
    assert_eq!(dry_run_info.to, TEST_SOLANA_ADDRESS);
    assert!(dry_run_info.estimated_fee.is_some());
}

#[test]
fn test_solana_provider_dry_run_without_config() {
    let config = Config {
        evm: None,
        solana: None,
        ..Default::default()
    };

    let v1_req = PaymentRequirementBuilder::solana()
        .amount("1000000")
        .build();
    let requirement = PaymentRequirements::V1(v1_req);

    let provider = PROVIDER_REGISTRY.find_provider("solana").unwrap();
    let result = provider.dry_run(&requirement, &config);

    assert!(result.is_err());
}

// =============================================================================
// Address Validation Tests
// =============================================================================

mod address_validation_tests {
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    #[test]
    fn test_valid_solana_addresses() {
        let valid_addresses = vec![
            "11111111111111111111111111111111",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        ];

        for addr in valid_addresses {
            let result = Pubkey::from_str(addr);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_invalid_solana_addresses() {
        let invalid_addresses = vec![
            ("invalid", "contains invalid characters"),
            ("0x1234567890123456789012345678901234567890", "EVM format"),
            ("", "empty string"),
            ("tooshort", "too short"),
        ];

        for (addr, _reason) in invalid_addresses {
            let result = Pubkey::from_str(addr);
            assert!(result.is_err());
        }
    }
}

// =============================================================================
// SPL Token Tests
// =============================================================================

mod spl_token_tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    #[test]
    fn test_spl_token_program_id() {
        let spl_token_id = spl_token::id();
        assert_eq!(spl_token_id, Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap());
    }

    #[test]
    fn test_spl_token_2022_program_id() {
        let spl_token_2022_id = spl_token_2022::id();
        assert_eq!(
            spl_token_2022_id,
            Pubkey::from_str(SPL_TOKEN_2022_PROGRAM).unwrap()
        );
    }

    #[test]
    fn test_associated_token_account_derivation() {
        let wallet = Pubkey::from_str(TEST_SOLANA_ADDRESS).unwrap();
        let mint = Pubkey::from_str(USDC_SOLANA).unwrap();

        let ata = spl_associated_token_account::get_associated_token_address(&wallet, &mint);
        let ata_str = ata.to_string();

        assert!(ata_str.len() >= 32);
        assert_ne!(ata, wallet);
    }

    #[test]
    fn test_ata_is_deterministic() {
        let wallet = Pubkey::from_str(TEST_SOLANA_ADDRESS).unwrap();
        let mint = Pubkey::from_str(USDC_SOLANA).unwrap();

        let ata1 = spl_associated_token_account::get_associated_token_address(&wallet, &mint);
        let ata2 = spl_associated_token_account::get_associated_token_address(&wallet, &mint);

        assert_eq!(ata1, ata2);
    }
}

// =============================================================================
// RPC URL Tests
// =============================================================================

mod rpc_url_tests {
    use purl_lib::network::{get_network, Network};
    use std::str::FromStr;

    #[test]
    fn test_solana_mainnet_rpc_url() {
        let network = get_network("solana").unwrap();
        assert!(network.rpc_url.contains("mainnet"));
    }

    #[test]
    fn test_solana_devnet_rpc_url() {
        let network = get_network("solana-devnet").unwrap();
        assert!(network.rpc_url.contains("devnet"));
    }

    #[test]
    fn test_network_from_str() {
        let mainnet = Network::from_str("solana");
        assert!(mainnet.is_ok());

        let devnet = Network::from_str("solana-devnet");
        assert!(devnet.is_ok());

        let unknown = Network::from_str("unknown-network");
        assert!(unknown.is_err());
    }

    #[test]
    fn test_network_from_str_error_message() {
        let result = Network::from_str("totally-fake-network");
        assert!(result.is_err());
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod proptest_tests {
    use proptest::prelude::*;
    use solana_sdk::pubkey::Pubkey;

    proptest! {
        #[test]
        fn test_random_bytes_as_pubkey(bytes in proptest::collection::vec(any::<u8>(), 32)) {
            let bytes_array: [u8; 32] = bytes.try_into().unwrap();
            let pubkey = Pubkey::new_from_array(bytes_array);
            assert_eq!(pubkey.to_bytes().len(), 32);
        }
    }
}
