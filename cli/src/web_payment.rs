//! Web Payment Auth protocol handling for the CLI
//!
//! This module handles the IETF Web Payment Auth protocol (draft-ietf-httpauth-payment-01)
//! which uses WWW-Authenticate and Authorization headers for blockchain payments.

use anyhow::{Context, Result};
use dialoguer::Confirm;
use purl_lib::protocol::web::{
    parse_receipt, parse_www_authenticate, ChargeRequest, PaymentChallenge,
};
use purl_lib::{Config, HttpResponse, Network, PROVIDER_REGISTRY};
use std::str::FromStr;

use crate::cli::Cli;
use crate::exit_codes::ExitCode;
use crate::request::RequestContext;
use purl_lib::utils::truncate_address;

/// Handle Web Payment Auth protocol (402 with WWW-Authenticate: Payment header)
pub async fn handle_web_payment_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    initial_response: &HttpResponse,
) -> Result<HttpResponse> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| anyhow::anyhow!("Missing WWW-Authenticate header in 402 response"))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Challenge ID: {}", challenge.id);
        eprintln!("Payment method: {}", challenge.method);
        eprintln!("Payment intent: {}", challenge.intent);
        if let Some(ref expires) = challenge.expires {
            eprintln!("Expires: {}", expires);
        }
    }

    let charge_req: ChargeRequest = serde_json::from_value(challenge.request.clone())
        .context("Failed to parse charge request from challenge")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Amount: {} (atomic units)", charge_req.amount);
        eprintln!("Asset: {}", charge_req.asset);
        eprintln!("Destination: {}", charge_req.destination);
    }

    challenge
        .validate()
        .context("Challenge validation failed")?;

    validate_web_payment_constraints(&request_ctx.cli, &challenge, &charge_req)?;

    if request_ctx.cli.dry_run {
        return handle_web_dry_run(config, &challenge, &charge_req);
    }

    if request_ctx.cli.confirm {
        confirm_web_payment(config, &challenge, &charge_req)?;
    }

    let network = challenge
        .network_name()
        .context("Failed to determine network from payment method")?;
    let provider = PROVIDER_REGISTRY
        .find_provider(network)
        .ok_or_else(|| anyhow::anyhow!("No provider found for network: {}", network))?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Creating payment credential...");
    }

    let credential = provider
        .create_web_payment(&challenge, config)
        .await
        .context("Failed to create payment credential")?;

    let auth_header = purl_lib::protocol::web::format_authorization(&credential)
        .context("Failed to format Authorization header")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
        eprintln!("Submitting payment to server...");
    }

    let payment_headers = vec![("Authorization".to_string(), auth_header)];
    let response = request_ctx.execute(url, Some(&payment_headers))?;

    display_web_receipt(&request_ctx.cli, &response)?;

    Ok(response)
}

fn validate_web_payment_constraints(
    cli: &Cli,
    challenge: &PaymentChallenge,
    charge_req: &ChargeRequest,
) -> Result<()> {
    if let Some(ref max_amount) = cli.max_amount {
        charge_req
            .validate_max_amount(max_amount)
            .context("Amount validation failed")?;
    }

    if let Some(ref networks) = cli.network {
        let allowed: Vec<&str> = networks.split(',').map(|s| s.trim()).collect();
        let network = challenge
            .network_name()
            .context("Failed to determine network from payment method")?;

        anyhow::ensure!(
            allowed.contains(&network),
            "Network '{}' not in allowed networks: {:?}",
            network,
            allowed
        );
    }

    Ok(())
}

/// Handle dry-run mode for web payments
fn handle_web_dry_run(
    config: &Config,
    challenge: &PaymentChallenge,
    charge_req: &ChargeRequest,
) -> Result<HttpResponse> {
    let network = challenge
        .network_name()
        .context("Failed to determine network from payment method")?;
    let provider = PROVIDER_REGISTRY
        .find_provider(network)
        .ok_or_else(|| anyhow::anyhow!("No provider found for network: {}", network))?;

    let from_address = provider.get_address(config)?;

    println!("[DRY RUN] Web Payment would be made:");
    println!("Protocol: Web Payment Auth");
    println!("Method: {}", challenge.method);
    println!("Intent: {}", challenge.intent);
    println!("Network: {}", network);
    println!("Amount: {} (atomic units)", charge_req.amount);
    println!("Asset: {}", charge_req.asset);
    println!("From: {}", from_address);
    println!("To: {}", charge_req.destination);
    if let Some(ref expires) = challenge.expires {
        println!("Expires: {}", expires);
    }

    anyhow::bail!("Dry run completed");
}

/// Confirm payment with user for web payments
fn confirm_web_payment(
    config: &Config,
    challenge: &PaymentChallenge,
    charge_req: &ChargeRequest,
) -> Result<()> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Cannot confirm payment: not running in an interactive terminal.\n\
             Remove --confirm flag or run in an interactive terminal."
        );
    }

    let network_name = challenge
        .network_name()
        .context("Failed to determine network from payment method")?;
    let provider = PROVIDER_REGISTRY.find_provider(network_name);
    let from_address = provider
        .and_then(|p| p.get_address(config).ok())
        .unwrap_or_else(|| "unknown".to_string());

    let token_config = Network::from_str(network_name)
        .map_err(|e| anyhow::anyhow!("Unknown network '{}': {}", network_name, e))?
        .require_usdc_config()
        .context("Cannot display formatted payment amount")?;
    let (decimals, symbol) = (token_config.currency.decimals, token_config.currency.symbol);

    let amount_u128: u128 = charge_req.amount.parse().unwrap_or(0);
    let divisor = 10u128.pow(decimals as u32) as f64;
    let amount_display = format!("{:.6} {}", amount_u128 as f64 / divisor, symbol);

    eprintln!();
    eprintln!("┌─────────────────────────────────────────────────────────────┐");
    eprintln!("│                  Web Payment Details                        │");
    eprintln!("├─────────────────────────────────────────────────────────────┤");
    eprintln!("│  Amount:    {:<47} │", amount_display);
    eprintln!(
        "│  Asset:     {:<47} │",
        truncate_address(&charge_req.asset, 45)
    );
    eprintln!("│  Network:   {:<47} │", network_name);
    eprintln!(
        "│  To:        {:<47} │",
        truncate_address(&charge_req.destination, 45)
    );
    eprintln!(
        "│  From:      {:<47} │",
        truncate_address(&from_address, 45)
    );
    eprintln!("└─────────────────────────────────────────────────────────────┘");
    eprintln!();

    let confirm = Confirm::new()
        .with_prompt("Proceed with this payment?")
        .default(false)
        .interact()?;

    if !confirm {
        ExitCode::UserCancelled.exit();
    }

    Ok(())
}

/// Display receipt information from response
fn display_web_receipt(cli: &Cli, response: &HttpResponse) -> Result<()> {
    if let Some(receipt_header) = response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_header) {
            if cli.is_verbose() && cli.should_show_output() {
                eprintln!("Payment receipt:");
                eprintln!("  Status: {}", receipt.status);
                eprintln!("  Method: {}", receipt.method);
                eprintln!("  TX Hash: {}", receipt.reference);
                eprintln!("  Timestamp: {}", receipt.timestamp);
                if let Some(ref block) = receipt.block_number {
                    eprintln!("  Block: {}", block);
                }
                if let Some(ref error) = receipt.error {
                    eprintln!("  Error: {}", error);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use purl_lib::protocol::web::{PaymentChallenge, PaymentIntent, PaymentMethod};

    fn mock_challenge(method: PaymentMethod, amount: &str) -> (PaymentChallenge, ChargeRequest) {
        let charge_req = ChargeRequest {
            amount: amount.to_string(),
            asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            destination: "0x1234567890123456789012345678901234567890".to_string(),
            expires: "2099-12-31T23:59:59Z".to_string(),
            fee_payer: None,
        };

        let challenge = PaymentChallenge {
            id: "test-challenge-id".to_string(),
            realm: "api.example.com".to_string(),
            method,
            intent: PaymentIntent::Charge,
            request: serde_json::to_value(&charge_req).unwrap(),
            description: None,
            expires: None,
        };

        (challenge, charge_req)
    }

    #[test]
    fn test_validate_constraints_no_constraints() {
        let cli = Cli::try_parse_from(["purl"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_ok() {
        let cli = Cli::try_parse_from(["purl", "--max-amount", "2000000"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_exceeded() {
        let cli = Cli::try_parse_from(["purl", "--max-amount", "500000"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Amount validation failed") || err.contains("exceeds"),
            "Error should mention amount validation: {}",
            err
        );
    }

    #[test]
    fn test_validate_constraints_max_amount_equal() {
        let cli = Cli::try_parse_from(["purl", "--max-amount", "1000000"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_match() {
        // PaymentMethod::Base maps to "base-sepolia" network
        let cli = Cli::try_parse_from(["purl", "--network", "base-sepolia"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_no_match() {
        let cli = Cli::try_parse_from(["purl", "--network", "solana"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not in allowed networks"),
            "Error should mention network filter: {}",
            err
        );
    }

    #[test]
    fn test_validate_constraints_multiple_networks() {
        // PaymentMethod::Base maps to "base-sepolia"
        let cli =
            Cli::try_parse_from(["purl", "--network", "solana, base-sepolia, ethereum"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_tempo_network() {
        // PaymentMethod::Tempo maps to "tempo-moderato" network
        let cli = Cli::try_parse_from(["purl", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Tempo, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_combined() {
        // PaymentMethod::Base maps to "base-sepolia"
        let cli = Cli::try_parse_from([
            "purl",
            "--max-amount",
            "2000000",
            "--network",
            "base-sepolia",
        ])
        .unwrap();
        let (challenge, charge_req) = mock_challenge(PaymentMethod::Base, "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }
}
