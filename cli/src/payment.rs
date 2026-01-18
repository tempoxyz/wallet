//! Payment handling for the CLI

use anyhow::{Context, Result};
use base64::Engine;
use dialoguer::Confirm;
use purl_lib::protocol::x402::{
    PaymentPayload, PaymentRequirements, PAYMENT_RESPONSE_HEADER, V1_X_PAYMENT_RESPONSE_HEADER,
};
use purl_lib::{
    Config, HttpResponse, PaymentRequirementsResponse, SettlementResponse, PROVIDER_REGISTRY,
};

use crate::cli::Cli;
use crate::exit_codes::ExitCode;
use crate::request::RequestContext;
use purl_lib::utils::truncate_address;

/// Handle payment required (402) response
pub async fn handle_payment_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    requirements: PaymentRequirementsResponse,
) -> Result<HttpResponse> {
    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("x402 version: {}", requirements.version());
        if let Some(error) = requirements.error() {
            eprintln!("Message: {}", error);
        }
        eprintln!(
            "Available payment methods: {}",
            requirements.accepts().len()
        );
    }

    let selected_requirement = select_payment_requirement(config, &requirements, &request_ctx.cli)?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        let amount = selected_requirement
            .parse_max_amount()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| "invalid".to_string());
        eprintln!(
            "Selected payment method: {} on {} ({})",
            selected_requirement.scheme(),
            selected_requirement.network(),
            amount
        );
    }

    if request_ctx.cli.dry_run {
        return handle_dry_run(config, &selected_requirement);
    }

    if request_ctx.cli.confirm {
        confirm_payment(config, &selected_requirement)?;
    }

    let payment_payload = create_payment_payload(config, &selected_requirement).await?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Created payment payload");
        eprintln!(
            "{} header (decoded):",
            payment_payload.payment_header_name()
        );
        if let Ok(pretty_json) = serde_json::to_string_pretty(&payment_payload) {
            eprintln!("{pretty_json}");
        }
        eprintln!("Making payment request...");
    }

    let response = request_ctx.execute_with_payment(url, &payment_payload)?;

    display_settlement_info(&request_ctx.cli, &response)?;

    Ok(response)
}

/// Handle dry-run mode
fn handle_dry_run(config: &Config, requirement: &PaymentRequirements) -> Result<HttpResponse> {
    let registry = &*PROVIDER_REGISTRY;
    if let Some(provider) = registry.find_provider(requirement.network()) {
        let dry_run_info = provider.dry_run(requirement, config)?;

        println!("[DRY RUN] Payment would be made:");
        println!("Provider: {}", dry_run_info.provider);
        println!("Network: {}", dry_run_info.network);
        println!("Amount: {} {}", dry_run_info.amount, dry_run_info.asset);
        println!("From: {}", dry_run_info.from);
        println!("To: {}", dry_run_info.to);
        if let Some(fee) = dry_run_info.estimated_fee {
            println!("Estimated Fee: {fee}");
        }

        anyhow::bail!("Dry run completed");
    } else {
        anyhow::bail!("No provider found for network: {}", requirement.network());
    }
}

/// Confirm payment with user
fn confirm_payment(config: &Config, requirement: &PaymentRequirements) -> Result<()> {
    use std::io::IsTerminal;

    // Check if we're in an interactive terminal
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Cannot confirm payment: not running in an interactive terminal.\n\
             Remove --confirm flag or run in an interactive terminal."
        );
    }

    // Format the amount for display
    let (amount_display, asset_symbol) = format_payment_amount(requirement);

    // Get sender address if available
    let from_address = get_sender_address(config, requirement);

    // Print payment details
    eprintln!();
    eprintln!("┌─────────────────────────────────────────────────────────────┐");
    eprintln!("│                     Payment Details                         │");
    eprintln!("├─────────────────────────────────────────────────────────────┤");
    eprintln!("│  Amount:    {:<47} │", amount_display);
    eprintln!("│  Asset:     {:<47} │", asset_symbol);
    eprintln!("│  Network:   {:<47} │", requirement.network());
    eprintln!(
        "│  To:        {:<47} │",
        truncate_address(requirement.pay_to(), 45)
    );
    if let Some(ref from) = from_address {
        eprintln!("│  From:      {:<47} │", truncate_address(from, 45));
    }
    eprintln!("└─────────────────────────────────────────────────────────────┘");
    eprintln!();

    let confirm = Confirm::new()
        .with_prompt("Proceed with this payment?")
        .default(false) // Default to no for safety
        .interact()?;

    if !confirm {
        ExitCode::UserCancelled.exit();
    }
    Ok(())
}

/// Format the payment amount for display
fn format_payment_amount(requirement: &PaymentRequirements) -> (String, String) {
    let amount_result = requirement.parse_max_amount();
    let asset = requirement.asset();
    let network = requirement.network();

    // Try to get decimals for the asset from the centralized token registry
    if let Ok(decimals) = purl_lib::constants::get_token_decimals(network, asset) {
        let amount_u128: u128 = amount_result.map(|a| a.as_atomic_units()).unwrap_or(0);
        let divisor = 10u128.pow(decimals as u32);
        let whole = amount_u128 / divisor;
        let frac = amount_u128 % divisor;

        // Get token symbol from centralized registry
        let symbol = purl_lib::constants::get_token_symbol(network, asset).unwrap_or("tokens");

        let amount_str = if frac == 0 {
            format!("{whole} {symbol}")
        } else {
            format!(
                "{whole}.{frac:0>width$} {symbol}",
                width = decimals as usize
            )
        };

        (amount_str, symbol.to_string())
    } else {
        // Fallback to raw amount
        let amount_str = amount_result
            .map(|a| format!("{} (atomic units)", a.as_atomic_units()))
            .unwrap_or_else(|_| "invalid amount".to_string());
        (amount_str, truncate_address(asset, 20))
    }
}

/// Get the sender address from config
fn get_sender_address(config: &Config, requirement: &PaymentRequirements) -> Option<String> {
    use purl_lib::WalletConfig;

    // Determine chain type from network
    if purl_lib::network::is_evm_network(requirement.network()) {
        config.evm.as_ref().and_then(|evm| evm.get_address().ok())
    } else if purl_lib::network::is_solana_network(requirement.network()) {
        config
            .solana
            .as_ref()
            .and_then(|sol| sol.get_address().ok())
    } else {
        None
    }
}

/// Display settlement information from response
fn display_settlement_info(cli: &Cli, response: &HttpResponse) -> Result<()> {
    // Check both v2 and v1 response headers
    let settlement_header = response
        .get_header(PAYMENT_RESPONSE_HEADER)
        .or_else(|| response.get_header(V1_X_PAYMENT_RESPONSE_HEADER));

    if let Some(settlement_header) = settlement_header {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(settlement_header)
            .context("Failed to decode payment response header")?;
        let settlement: SettlementResponse =
            serde_json::from_slice(&decoded).context("Failed to parse settlement response")?;

        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Payment settled:");
            eprintln!("Transaction: {}", settlement.transaction());
            eprintln!("Network: {}", settlement.network());
            eprintln!("Success: {}", settlement.is_success());
            if let Some(payer) = settlement.payer() {
                eprintln!("Payer: {payer}");
            }
            if let Some(reason) = settlement.error_reason() {
                eprintln!("Error: {reason}");
            }
        }
    }
    Ok(())
}

/// Select the best compatible payment requirement from server's response.
fn select_payment_requirement(
    config: &Config,
    requirements: &PaymentRequirementsResponse,
    cli: &Cli,
) -> Result<PaymentRequirements> {
    use purl_lib::negotiator::PaymentNegotiator;

    let allowed_networks = cli.allowed_networks().unwrap_or_default();

    let negotiator = PaymentNegotiator::new(config)
        .with_allowed_networks(&allowed_networks)
        .with_max_amount(cli.max_amount.as_deref());

    negotiator
        .select_from_requirements(requirements)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Create a payment payload for the selected requirement
async fn create_payment_payload(
    config: &Config,
    requirement: &PaymentRequirements,
) -> Result<PaymentPayload> {
    let registry = &*PROVIDER_REGISTRY;

    if let Some(provider) = registry.find_provider(requirement.network()) {
        Ok(provider.create_payment(requirement, config).await?)
    } else {
        anyhow::bail!("No provider found for network: {}", requirement.network());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purl_lib::protocol::x402::v1;
    use purl_lib::{EvmConfig, SolanaConfig};

    /// Test EVM private key (DO NOT use in production)
    const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

    /// Test Solana private key (DO NOT use in production)
    const TEST_SOLANA_KEY: &str =
        "4Z7cXSyeFR8wNGMVXUE1TwtKn5D5Vu7FzEv69dokLv7KrQk7h6pu4LF8ZRR9yQBhc7uSM6RTTZtU1fmaxiNrxXrs";

    fn mock_evm_requirement() -> PaymentRequirements {
        let v1_req = v1::PaymentRequirements {
            scheme: "exact".to_string(),
            network: "base".to_string(),
            max_amount_required: "1000000".to_string(), // 1 USDC
            resource: "https://example.com/resource".to_string(),
            description: "Test payment".to_string(),
            mime_type: "application/json".to_string(),
            output_schema: None,
            pay_to: "0x1234567890123456789012345678901234567890".to_string(),
            max_timeout_seconds: 300,
            asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(), // USDC on Base
            extra: Some(serde_json::json!({
                "name": "USD Coin",
                "version": "2"
            })),
        };
        PaymentRequirements::V1(v1_req)
    }

    fn mock_solana_requirement() -> PaymentRequirements {
        let v1_req = v1::PaymentRequirements {
            scheme: "exact".to_string(),
            network: "solana".to_string(),
            max_amount_required: "1000000".to_string(), // 1 USDC
            resource: "https://example.com/resource".to_string(),
            description: "Test payment".to_string(),
            mime_type: "application/json".to_string(),
            output_schema: None,
            pay_to: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            max_timeout_seconds: 300,
            asset: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC on Solana
            extra: None,
        };
        PaymentRequirements::V1(v1_req)
    }

    fn config_with_evm() -> Config {
        Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        }
    }

    fn config_with_solana() -> Config {
        Config {
            evm: None,
            solana: Some(SolanaConfig {
                keystore: None,
                private_key: Some(TEST_SOLANA_KEY.to_string()),
            }),
            ..Default::default()
        }
    }

    fn config_with_both() -> Config {
        Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: Some(SolanaConfig {
                keystore: None,
                private_key: Some(TEST_SOLANA_KEY.to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_format_payment_amount_usdc() {
        let req = mock_evm_requirement();
        let (amount_str, symbol) = format_payment_amount(&req);

        // 1000000 atomic units = 1 USDC (6 decimals)
        assert_eq!(symbol, "USDC");
        assert!(
            amount_str.contains("USDC"),
            "Amount should contain USDC symbol"
        );
    }

    #[test]
    fn test_format_payment_amount_large_amount() {
        let v1_req = v1::PaymentRequirements {
            scheme: "exact".to_string(),
            network: "base".to_string(),
            max_amount_required: "1000000000".to_string(), // 1000 USDC
            resource: "https://example.com".to_string(),
            description: "".to_string(),
            mime_type: "application/json".to_string(),
            output_schema: None,
            pay_to: "0x1234567890123456789012345678901234567890".to_string(),
            max_timeout_seconds: 300,
            asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            extra: None,
        };
        let req = PaymentRequirements::V1(v1_req);
        let (amount_str, symbol) = format_payment_amount(&req);

        assert_eq!(symbol, "USDC");
        assert!(amount_str.contains("1000"));
    }

    #[test]
    fn test_get_sender_address_evm() {
        let config = config_with_evm();
        let req = mock_evm_requirement();

        let address = get_sender_address(&config, &req);
        assert!(address.is_some());

        let addr = address.unwrap();
        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42);
    }

    #[test]
    fn test_get_sender_address_solana() {
        let config = config_with_solana();
        let req = mock_solana_requirement();

        let address = get_sender_address(&config, &req);
        assert!(address.is_some());

        let addr = address.unwrap();
        // Solana addresses are base58 encoded, typically 32-44 characters
        assert!(addr.len() >= 32 && addr.len() <= 44);
    }

    #[test]
    fn test_get_sender_address_no_matching_config() {
        // EVM config but Solana requirement
        let config = config_with_evm();
        let req = mock_solana_requirement();

        let address = get_sender_address(&config, &req);
        assert!(address.is_none());
    }

    #[test]
    fn test_get_sender_address_empty_config() {
        let config = Config::default();
        let req = mock_evm_requirement();

        let address = get_sender_address(&config, &req);
        assert!(address.is_none());
    }

    #[test]
    fn test_get_sender_address_both_configs() {
        let config = config_with_both();

        // Should return EVM address for EVM requirement
        let evm_req = mock_evm_requirement();
        let evm_address = get_sender_address(&config, &evm_req);
        assert!(evm_address.is_some());
        assert!(evm_address.unwrap().starts_with("0x"));

        // Should return Solana address for Solana requirement
        let sol_req = mock_solana_requirement();
        let sol_address = get_sender_address(&config, &sol_req);
        assert!(sol_address.is_some());
        assert!(!sol_address.unwrap().starts_with("0x"));
    }
}
