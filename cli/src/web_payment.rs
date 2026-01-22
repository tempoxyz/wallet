//! Web Payment Auth protocol handling for the CLI
//!
//! This module handles the IETF Web Payment Auth protocol (draft-ietf-httpauth-payment-01)
//! which uses WWW-Authenticate and Authorization headers for blockchain payments.

use anyhow::{Context, Result};
use dialoguer::Confirm;
use purl::explorer::ExplorerConfig;
use purl::protocol::web::{parse_receipt, parse_www_authenticate, ChargeRequest, PaymentChallenge};
use purl::{Config, HttpResponse, Network, PROVIDER_REGISTRY};
use std::str::FromStr;

use crate::cli::Cli;
use crate::exit_codes::ExitCode;
use crate::hyperlink::hyperlink;
use crate::request::RequestContext;
use purl::utils::truncate_address;

/// Format an address as a clickable hyperlink if explorer is available.
fn format_address_link(address: &str, explorer: Option<&ExplorerConfig>) -> String {
    if let Some(exp) = explorer {
        let url = exp.address_url(address);
        hyperlink(address, &url)
    } else {
        address.to_string()
    }
}

/// Format and truncate an address as a clickable hyperlink if explorer is available.
/// The displayed text is truncated but the link points to the full address.
fn format_truncated_address_link(
    address: &str,
    max_len: usize,
    explorer: Option<&ExplorerConfig>,
) -> String {
    let truncated = truncate_address(address, max_len);
    if let Some(exp) = explorer {
        let url = exp.address_url(address);
        hyperlink(&truncated, &url)
    } else {
        truncated
    }
}

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
    let network = challenge
        .network_name()
        .context("Failed to determine network from payment method")?;
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

    let charge_req: ChargeRequest = serde_json::from_value(challenge.request.clone())
        .context("Failed to parse charge request from challenge")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Amount: {} (atomic units)", charge_req.amount);
        eprintln!(
            "Asset: {}",
            format_address_link(&charge_req.asset, explorer.as_ref())
        );
        eprintln!(
            "Destination: {}",
            format_address_link(&charge_req.destination, explorer.as_ref())
        );
    }

    challenge
        .validate()
        .context("Challenge validation failed")?;

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

    let auth_header = purl::protocol::web::format_authorization(&credential)
        .context("Failed to format Authorization header")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
        eprintln!("Submitting payment to server...");
    }

    let payment_headers = vec![("Authorization".to_string(), auth_header)];
    let response = request_ctx.execute(url, Some(&payment_headers))?;

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
    explorer: Option<&ExplorerConfig>,
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
    println!("Asset: {}", format_address_link(&charge_req.asset, explorer));
    println!("From: {}", format_address_link(&from_address, explorer));
    println!(
        "To: {}",
        format_address_link(&charge_req.destination, explorer)
    );
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
    explorer: Option<&ExplorerConfig>,
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

    // Format addresses with clickable links (truncated for display, full URL)
    let asset_display = format_truncated_address_link(&charge_req.asset, 45, explorer);
    let to_display = format_truncated_address_link(&charge_req.destination, 45, explorer);
    let from_display = format_truncated_address_link(&from_address, 45, explorer);

    eprintln!();
    eprintln!("┌─────────────────────────────────────────────────────────────┐");
    eprintln!("│                  Web Payment Details                        │");
    eprintln!("├─────────────────────────────────────────────────────────────┤");
    eprintln!("│  Amount:    {:<47} │", amount_display);
    eprintln!("│  Asset:     {} │", pad_with_hyperlink(&asset_display, 47));
    eprintln!("│  Network:   {:<47} │", network_name);
    eprintln!("│  To:        {} │", pad_with_hyperlink(&to_display, 47));
    eprintln!("│  From:      {} │", pad_with_hyperlink(&from_display, 47));
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

/// Pad a string containing possible hyperlink escape codes to a target visible width.
/// Hyperlink escape codes don't contribute to visible width.
fn pad_with_hyperlink(s: &str, width: usize) -> String {
    // Count visible characters (excluding ANSI escape sequences)
    let visible_len = strip_ansi_codes_len(s);
    if visible_len >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - visible_len))
    }
}

/// Count the visible length of a string, excluding ANSI escape codes.
fn strip_ansi_codes_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            in_escape = true;
            // Check for OSC sequence (ESC ])
            if chars.peek() == Some(&']') {
                chars.next(); // consume ]
                // Skip until BEL (\x07) or ST (ESC \)
                while let Some(c2) = chars.next() {
                    if c2 == '\x07' {
                        break;
                    }
                    if c2 == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
                in_escape = false;
            }
        } else if in_escape {
            // CSI sequence ends at letter
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            len += 1;
        }
    }
    len
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
    use purl::protocol::web::{PaymentChallenge, PaymentIntent, PaymentMethod};

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
        let cli = Cli::try_parse_from(["purl", "--network", "ethereum"]).unwrap();
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
        let cli = Cli::try_parse_from(["purl", "--network", "base-sepolia, ethereum"]).unwrap();
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

    #[test]
    fn test_strip_ansi_codes_len_plain_text() {
        assert_eq!(strip_ansi_codes_len("hello"), 5);
        assert_eq!(strip_ansi_codes_len("0x1234567890"), 12);
    }

    #[test]
    fn test_strip_ansi_codes_len_with_osc8() {
        // OSC 8 hyperlink format: ESC ] 8 ; ; URL BEL text ESC ] 8 ; ; BEL
        let hyperlink = "\x1b]8;;https://example.com\x07click me\x1b]8;;\x07";
        assert_eq!(strip_ansi_codes_len(hyperlink), 8); // "click me" = 8 chars
    }

    #[test]
    fn test_pad_with_hyperlink_plain() {
        let result = pad_with_hyperlink("hello", 10);
        assert_eq!(result, "hello     ");
    }

    #[test]
    fn test_pad_with_hyperlink_with_escape() {
        let hyperlink = "\x1b]8;;https://example.com\x07text\x1b]8;;\x07";
        let result = pad_with_hyperlink(hyperlink, 10);
        // "text" is 4 chars, so we need 6 spaces
        assert_eq!(
            result,
            "\x1b]8;;https://example.com\x07text\x1b]8;;\x07      "
        );
    }
}
