//! Tests for CLI payment handling functions

mod common;

use common::{
    format_amount_display, test_command, truncate_address, PaymentRequirementBuilder,
    TestConfigBuilder, TEST_EVM_KEY, TEST_SOLANA_KEY,
};

// =============================================================================
// Amount Formatting Tests
// =============================================================================

mod format_payment_amount_tests {
    use super::*;

    #[test]
    fn test_format_whole_number() {
        assert_eq!(format_amount_display(1_000_000, 6, "USDC"), "1 USDC");
        assert_eq!(format_amount_display(10_000_000, 6, "USDC"), "10 USDC");
        assert_eq!(format_amount_display(1_000_000_000, 6, "USDC"), "1000 USDC");
    }

    #[test]
    fn test_format_fractional_amount() {
        assert_eq!(format_amount_display(1_500_000, 6, "USDC"), "1.500000 USDC");
        assert_eq!(format_amount_display(10_000, 6, "USDC"), "0.010000 USDC");
        assert_eq!(format_amount_display(1, 6, "USDC"), "0.000001 USDC");
    }

    #[test]
    fn test_format_zero_amount() {
        assert_eq!(format_amount_display(0, 6, "USDC"), "0 USDC");
    }

    #[test]
    fn test_format_large_amount() {
        assert_eq!(
            format_amount_display(1_000_000_000_000_000, 6, "USDC"),
            "1000000000 USDC"
        );
    }

    #[test]
    fn test_format_different_decimals() {
        let one_eth = 10u128.pow(18);
        assert_eq!(format_amount_display(one_eth, 18, "ETH"), "1 ETH");

        let one_sol = 10u128.pow(9);
        assert_eq!(format_amount_display(one_sol, 9, "SOL"), "1 SOL");
    }
}

// =============================================================================
// Address Truncation Tests
// =============================================================================

mod truncate_address_tests {
    use super::*;

    #[test]
    fn test_short_address_unchanged() {
        let short = "0x1234";
        assert_eq!(truncate_address(short, 45), short);
    }

    #[test]
    fn test_evm_address_truncation() {
        let evm_addr = "0x1234567890abcdef1234567890abcdef12345678";
        assert_eq!(truncate_address(evm_addr, 20), "0x1234...5678");
    }

    #[test]
    fn test_solana_address_truncation() {
        let solana_addr = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        assert_eq!(truncate_address(solana_addr, 20), "EPjFWd...Dt1v");
    }

    #[test]
    fn test_exact_max_length() {
        let addr = "1234567890";
        assert_eq!(truncate_address(addr, 10), addr);
    }

    #[test]
    fn test_one_over_max_length() {
        let addr = "12345678901";
        assert_eq!(truncate_address(addr, 10), "123456...8901");
    }

    #[test]
    fn test_empty_address() {
        assert_eq!(truncate_address("", 45), "");
    }
}

// =============================================================================
// Token Decimals Tests
// =============================================================================

mod token_decimals_tests {
    use purl_lib::constants::get_token_decimals;

    #[test]
    fn test_usdc_decimals_on_base() {
        let usdc_base = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        let result = get_token_decimals("base", usdc_base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_usdc_decimals_on_solana() {
        let usdc_solana = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        let result = get_token_decimals("solana", usdc_solana);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_unknown_token_decimals() {
        let result = get_token_decimals("base", "0x0000000000000000000000000000000000000000");
        assert!(result.is_err());
    }
}

// =============================================================================
// Token Symbol Tests
// =============================================================================

mod token_symbol_tests {
    use purl_lib::constants::get_token_symbol;

    #[test]
    fn test_usdc_symbol_on_base() {
        let usdc_base = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        assert_eq!(get_token_symbol("base", usdc_base), Some("USDC"));
    }

    #[test]
    fn test_usdc_symbol_on_ethereum() {
        let usdc_eth = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        assert_eq!(get_token_symbol("ethereum", usdc_eth), Some("USDC"));
    }

    #[test]
    fn test_unknown_token_symbol() {
        let result = get_token_symbol("base", "0x0000000000000000000000000000000000000000");
        assert_eq!(result, None);
    }
}

// =============================================================================
// Network Type Detection Tests
// =============================================================================

mod network_detection_tests {
    use purl_lib::network::{is_evm_network, is_solana_network};

    #[test]
    fn test_is_evm_network() {
        assert!(is_evm_network("base"));
        assert!(is_evm_network("base-sepolia"));
        assert!(is_evm_network("ethereum"));
        assert!(!is_evm_network("solana"));
        assert!(!is_evm_network("solana-devnet"));
        assert!(!is_evm_network("unknown"));
    }

    #[test]
    fn test_is_solana_network() {
        assert!(is_solana_network("solana"));
        assert!(is_solana_network("solana-devnet"));
        assert!(!is_solana_network("base"));
        assert!(!is_solana_network("ethereum"));
        assert!(!is_solana_network("unknown"));
    }
}

// =============================================================================
// Payment Negotiation Tests
// =============================================================================

mod payment_negotiation_tests {
    use super::*;
    use purl_lib::negotiator::PaymentNegotiator;
    use purl_lib::{Config, EvmConfig, SolanaConfig};

    #[test]
    fn test_negotiator_selects_evm_with_evm_config() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        };

        let json = PaymentRequirementBuilder::evm()
            .amount("1000000")
            .build_json();

        let negotiator = PaymentNegotiator::new(&config);
        let result = negotiator.select_requirement(&json);

        assert!(result.is_ok());
        let selected = result.unwrap();
        assert_eq!(selected.network(), "base");
    }

    #[test]
    fn test_negotiator_selects_solana_with_solana_config() {
        let config = Config {
            evm: None,
            solana: Some(SolanaConfig {
                keystore: None,
                private_key: Some(TEST_SOLANA_KEY.to_string()),
            }),
            ..Default::default()
        };

        let json = PaymentRequirementBuilder::solana()
            .amount("1000000")
            .build_json();

        let negotiator = PaymentNegotiator::new(&config);
        let result = negotiator.select_requirement(&json);

        assert!(result.is_ok());
        let selected = result.unwrap();
        assert_eq!(selected.network(), "solana");
    }

    #[test]
    fn test_negotiator_max_amount_constraint_exceeded() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        };

        let json = PaymentRequirementBuilder::evm()
            .amount("1000000")
            .build_json();

        let negotiator = PaymentNegotiator::new(&config).with_max_amount(Some("500000"));
        let result = negotiator.select_requirement(&json);

        assert!(result.is_err());
    }

    #[test]
    fn test_negotiator_max_amount_sufficient() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        };

        let json = PaymentRequirementBuilder::evm()
            .amount("1000000")
            .build_json();

        let negotiator = PaymentNegotiator::new(&config).with_max_amount(Some("2000000"));
        let result = negotiator.select_requirement(&json);

        assert!(result.is_ok());
    }

    #[test]
    fn test_negotiator_no_compatible_config() {
        let config = Config {
            evm: None,
            solana: None,
            ..Default::default()
        };

        let json = PaymentRequirementBuilder::evm()
            .amount("1000000")
            .build_json();

        let negotiator = PaymentNegotiator::new(&config);
        let result = negotiator.select_requirement(&json);

        assert!(result.is_err());
    }

    #[test]
    fn test_negotiator_network_filter() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        };

        let json = PaymentRequirementBuilder::evm()
            .network("base")
            .amount("1000000")
            .build_json();

        let negotiator =
            PaymentNegotiator::new(&config).with_allowed_networks(&["ethereum".to_string()]);
        let result = negotiator.select_requirement(&json);

        assert!(result.is_err());
    }
}

// =============================================================================
// CLI Output Tests
// =============================================================================

mod cli_payment_output_tests {
    use super::*;
    use assert_cmd::prelude::*;

    #[test]
    fn test_inspect_command_exists() {
        let temp_dir = TestConfigBuilder::new().with_default_evm().build();

        let mut cmd = test_command(&temp_dir);
        cmd.args(["inspect", "--help"]);

        cmd.assert().success();
    }

    #[test]
    fn test_dry_run_flag_in_help() {
        let temp_dir = TestConfigBuilder::new().with_default_evm().build();

        let mut cmd = test_command(&temp_dir);
        cmd.args(["--help"]);

        let output = cmd.output().expect("Failed to get output");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(stdout.contains("dry-run") || stdout.contains("dry_run"));
    }

    #[test]
    fn test_confirm_flag_in_help() {
        let temp_dir = TestConfigBuilder::new().with_default_evm().build();

        let mut cmd = test_command(&temp_dir);
        cmd.args(["--help"]);

        let output = cmd.output().expect("Failed to get output");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(stdout.contains("confirm"));
    }

    #[test]
    fn test_max_amount_flag_in_help() {
        let temp_dir = TestConfigBuilder::new().with_default_evm().build();

        let mut cmd = test_command(&temp_dir);
        cmd.args(["--help"]);

        let output = cmd.output().expect("Failed to get output");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(stdout.contains("max-amount") || stdout.contains("max_amount"));
    }

    #[test]
    fn test_network_flag_in_help() {
        let temp_dir = TestConfigBuilder::new().with_default_evm().build();

        let mut cmd = test_command(&temp_dir);
        cmd.args(["--help"]);

        let output = cmd.output().expect("Failed to get output");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(stdout.contains("network") || stdout.contains("allowed"));
    }
}
