//! Web Payment Auth protocol handling for the CLI
//!
//! This module handles the IETF Web Payment Auth protocol (draft-ietf-httpauth-payment-01)
//! which uses WWW-Authenticate and Authorization headers for blockchain payments.

use anyhow::{Context, Result};
use std::str::FromStr;

use mpay::Challenge::{parse_www_authenticate, PaymentChallenge};
use mpay::Intent::ChargeRequest;
use mpay::Receipt::parse_receipt;

use crate::cli::confirm::confirm_web_payment;
use crate::cli::formatting::format_address_link;
use crate::cli::hyperlink::hyperlink;
use crate::cli::Cli;
use crate::config::Config;
use crate::http::request::RequestContext;
use crate::http::HttpResponse;
use crate::network::explorer::ExplorerConfig;
use crate::network::Network;
use crate::payment::mpay_ext::{method_to_network, validate_challenge};
use crate::payment::provider::PROVIDER_REGISTRY;

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

    // Get network and explorer config early for clickable links
    let network = method_to_network(&challenge.method)
        .ok_or_else(|| anyhow::anyhow!("Unsupported payment method: {}", challenge.method))?;
    let explorer = Network::from_str(network)
        .ok()
        .and_then(|n| n.info().explorer);

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Challenge ID: {}", challenge.id);
        eprintln!("Payment method: {}", challenge.method);
        eprintln!("Payment intent: {}", challenge.intent);
        if let Some(ref expires) = challenge.expires {
            eprintln!("Expires: {}", expires);
        }
    }

    let charge_req: ChargeRequest = challenge
        .request
        .decode()
        .context("Failed to parse charge request from challenge")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Amount: {} (atomic units)", charge_req.amount);
        eprintln!(
            "Currency: {}",
            format_address_link(&charge_req.currency, explorer.as_ref())
        );
        if let Some(ref recipient) = charge_req.recipient {
            eprintln!(
                "Recipient: {}",
                format_address_link(recipient, explorer.as_ref())
            );
        }
    }

    validate_challenge(&challenge).context("Challenge validation failed")?;

    validate_web_payment_constraints(&request_ctx.cli, &challenge, &charge_req)?;

    if request_ctx.cli.dry_run {
        return handle_web_dry_run(config, &challenge, &charge_req, explorer.as_ref());
    }

    if request_ctx.cli.confirm {
        confirm_web_payment(config, &challenge, &charge_req, explorer.as_ref())?;
    }

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

    let auth_header = mpay::Credential::format_authorization(&credential)
        .context("Failed to format Authorization header")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
        eprintln!("Submitting payment to server...");
    }

    let payment_headers = vec![("Authorization".to_string(), auth_header)];
    let response = request_ctx.execute(url, Some(&payment_headers)).await?;

    display_web_receipt(&request_ctx.cli, &response, explorer.as_ref())?;

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
        let network = method_to_network(&challenge.method)
            .ok_or_else(|| anyhow::anyhow!("Unsupported payment method: {}", challenge.method))?;

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
    explorer: Option<&ExplorerConfig>,
) -> Result<HttpResponse> {
    let network = method_to_network(&challenge.method)
        .ok_or_else(|| anyhow::anyhow!("Unsupported payment method: {}", challenge.method))?;
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
    println!(
        "Currency: {}",
        format_address_link(&charge_req.currency, explorer)
    );
    println!("From: {}", format_address_link(&from_address, explorer));
    if let Some(ref recipient) = charge_req.recipient {
        println!("To: {}", format_address_link(recipient, explorer));
    }
    if let Some(ref expires) = challenge.expires {
        println!("Expires: {}", expires);
    }

    anyhow::bail!("Dry run completed");
}

/// Display receipt information from response with optional clickable explorer links.
fn display_web_receipt(
    cli: &Cli,
    response: &HttpResponse,
    explorer: Option<&ExplorerConfig>,
) -> Result<()> {
    if let Some(receipt_header) = response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_header) {
            if cli.is_verbose() && cli.should_show_output() {
                eprintln!("Payment receipt:");
                eprintln!("  Status: {}", receipt.status);
                eprintln!("  Method: {}", receipt.method);

                // TX Hash with optional clickable link
                let tx_display = if let Some(exp) = explorer {
                    let url = exp.tx_url(&receipt.reference);
                    hyperlink(&receipt.reference, &url)
                } else {
                    receipt.reference.clone()
                };
                eprintln!("  TX Hash: {}", tx_display);

                eprintln!("  Timestamp: {}", receipt.timestamp);

                // Block number with optional clickable link
                if let Some(ref block) = receipt.block_number {
                    let block_display = if let Some(exp) = explorer {
                        let url = exp.block_url(block);
                        hyperlink(block, &url)
                    } else {
                        block.clone()
                    };
                    eprintln!("  Block: {}", block_display);
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
    use mpay::Challenge::PaymentChallenge;
    use mpay::Schema::MethodName;

    fn mock_challenge(method: MethodName, amount: &str) -> (PaymentChallenge, ChargeRequest) {
        use mpay::Schema::Base64UrlJson;

        let charge_req = ChargeRequest {
            amount: amount.to_string(),
            currency: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            recipient: Some("0x1234567890123456789012345678901234567890".to_string()),
            expires: Some("2099-12-31T23:59:59Z".to_string()),
            description: None,
            external_id: None,
            method_details: None,
        };

        let challenge = PaymentChallenge {
            id: "test-challenge-id".to_string(),
            realm: "api.example.com".to_string(),
            method,
            intent: "charge".into(),
            request: Base64UrlJson::from_typed(&charge_req).expect("serialize charge request"),
            digest: None,
            description: None,
            expires: None,
        };

        (challenge, charge_req)
    }

    #[test]
    fn test_validate_constraints_no_constraints() {
        let cli = Cli::try_parse_from(["purl"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_ok() {
        let cli = Cli::try_parse_from(["purl", "--max-amount", "2000000"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_exceeded() {
        let cli = Cli::try_parse_from(["purl", "--max-amount", "500000"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

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
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_match() {
        // MethodName::new("tempo") maps to "tempo-moderato" network
        let cli = Cli::try_parse_from(["purl", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_no_match() {
        let cli = Cli::try_parse_from(["purl", "--network", "ethereum"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

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
        // MethodName::new("tempo") maps to "tempo-moderato"
        let cli = Cli::try_parse_from(["purl", "--network", "tempo-moderato, ethereum"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_tempo_network() {
        // MethodName::new("tempo") maps to "tempo-moderato" network
        let cli = Cli::try_parse_from(["purl", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_combined() {
        // MethodName::new("tempo") maps to "tempo-moderato"
        let cli = Cli::try_parse_from([
            "purl",
            "--max-amount",
            "2000000",
            "--network",
            "tempo-moderato",
        ])
        .unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }
}
