//! Web Payment Auth protocol handling for the CLI
//!
//! This module handles the IETF Web Payment Auth protocol (draft-ietf-httpauth-payment-01)
//! which uses WWW-Authenticate and Authorization headers for blockchain payments.
//!
//! # Security Model
//!
//! Origin validation ensures the challenge `realm` matches the request host,
//! preventing malicious servers from returning payment challenges for different origins.

use anyhow::{Context, Result};
use reqwest::Url;
use std::str::FromStr;

use mpay::{parse_receipt, parse_www_authenticate, ChargeRequest, PaymentChallenge};

use crate::cli::confirm::confirm_web_payment;
use crate::cli::formatting::format_address_link;
use crate::cli::hyperlink::hyperlink;
use crate::cli::{Cli, QueryArgs};
use crate::config::{Config, WalletConfig};
use crate::http::request::RequestContext;
use crate::http::HttpResponse;
use crate::network::explorer::ExplorerConfig;
use crate::network::Network;
use crate::payment::mpay_ext::{method_to_network, validate_challenge};
use crate::payment::provider::PgetPaymentProvider;

/// Validate that the challenge realm matches the request origin.
///
/// # Security
///
/// This prevents a malicious server from returning a challenge for a different
/// origin, which could trick the client into making a payment to an unintended
/// recipient. The realm should match the host of the original request.
fn validate_origin(url: &str, challenge: &PaymentChallenge, skip_check: bool) -> Result<()> {
    if skip_check {
        return Ok(());
    }

    let parsed_url = Url::parse(url).context("Failed to parse request URL")?;
    let request_host = parsed_url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Request URL has no host"))?;

    let realm_lower = challenge.realm.to_lowercase();
    let host_lower = request_host.to_lowercase();

    let is_valid = realm_lower == host_lower
        || realm_lower.ends_with(&format!(".{}", host_lower))
        || host_lower.ends_with(&format!(".{}", realm_lower));

    if !is_valid {
        anyhow::bail!(
            "Payment challenge realm '{}' does not match request host '{}'. \
             This could indicate a malicious server. \
             Use --insecure to bypass (DANGEROUS).",
            challenge.realm,
            request_host
        );
    }

    Ok(())
}

/// Handle Web Payment Auth protocol (402 with WWW-Authenticate: Payment header)
///
/// # Security Requirements
///
/// - Origin validation ensures challenge realm matches request host
/// - Additional constraints from CLI flags (max-amount, network) are enforced
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

    // SECURITY: Validate origin before proceeding
    validate_origin(url, &challenge, request_ctx.query.insecure)?;

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

    validate_web_payment_constraints(
        &request_ctx.query,
        &request_ctx.cli,
        &challenge,
        &charge_req,
    )?;

    if request_ctx.query.dry_run {
        return handle_web_dry_run(config, &challenge, &charge_req, explorer.as_ref());
    }

    if request_ctx.query.confirm {
        confirm_web_payment(config, &challenge, &charge_req, explorer.as_ref())?;
    }

    // Use mpay::client::PaymentProvider to create the credential
    let provider = PgetPaymentProvider::with_no_swap(config.clone(), request_ctx.query.no_swap);

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Creating payment credential...");
    }

    use mpay::client::PaymentProvider;
    let credential = provider
        .pay(&challenge)
        .await
        .context("Failed to create payment credential")?;

    let auth_header =
        mpay::format_authorization(&credential).context("Failed to format Authorization header")?;

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
    query: &QueryArgs,
    cli: &Cli,
    challenge: &PaymentChallenge,
    charge_req: &ChargeRequest,
) -> Result<()> {
    if let Some(ref max_amount) = query.max_amount {
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

    let from_address = config
        .require_evm()
        .and_then(|evm| evm.get_address())
        .unwrap_or_else(|_| "unknown".to_string());

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

                let tx_display = if let Some(exp) = explorer {
                    let url = exp.tx_url(&receipt.reference);
                    hyperlink(&receipt.reference, &url)
                } else {
                    receipt.reference.clone()
                };
                eprintln!("  TX Hash: {}", tx_display);

                eprintln!("  Timestamp: {}", receipt.timestamp);

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
    use crate::cli::test_utils::make_query_args;
    use clap::Parser;
    use mpay::{MethodName, PaymentChallenge};

    fn mock_challenge_with_realm(
        method: MethodName,
        amount: &str,
        realm: &str,
    ) -> (PaymentChallenge, ChargeRequest) {
        use mpay::Base64UrlJson;

        let charge_req = ChargeRequest {
            amount: amount.to_string(),
            currency: "0x20c0000000000000000000000000000000000001".to_string(),
            recipient: Some("0x1234567890123456789012345678901234567890".to_string()),
            expires: Some("2099-12-31T23:59:59Z".to_string()),
            description: None,
            external_id: None,
            method_details: None,
        };

        let challenge = PaymentChallenge {
            id: "test-challenge-id".to_string(),
            realm: realm.to_string(),
            method,
            intent: "charge".into(),
            request: Base64UrlJson::from_typed(&charge_req).expect("serialize charge request"),
            digest: None,
            description: None,
            expires: None,
        };

        (challenge, charge_req)
    }

    fn mock_challenge(method: MethodName, amount: &str) -> (PaymentChallenge, ChargeRequest) {
        mock_challenge_with_realm(method, amount, "api.example.com")
    }

    fn default_query() -> QueryArgs {
        make_query_args(&["query", "http://example.com"])
    }

    #[test]
    fn test_validate_origin_exact_match() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "api.example.com");
        let result = validate_origin("https://api.example.com/pay", &challenge, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_origin_case_insensitive() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "API.EXAMPLE.COM");
        let result = validate_origin("https://api.example.com/pay", &challenge, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_origin_subdomain_of_realm() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "example.com");
        let result = validate_origin("https://api.example.com/pay", &challenge, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_origin_realm_is_subdomain() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "api.example.com");
        let result = validate_origin("https://example.com/pay", &challenge, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_origin_mismatch() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "evil.com");
        let result = validate_origin("https://api.example.com/pay", &challenge, false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not match request host"));
        assert!(err.contains("evil.com"));
    }

    #[test]
    fn test_validate_origin_skip_check() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "evil.com");
        let result = validate_origin("https://api.example.com/pay", &challenge, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_origin_partial_match_rejected() {
        let (challenge, _) =
            mock_challenge_with_realm(MethodName::new("tempo"), "1000000", "example.com");
        let result = validate_origin("https://notexample.com/pay", &challenge, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_constraints_no_constraints() {
        let query = default_query();
        let cli = Cli::try_parse_from(["pget"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_ok() {
        let query = make_query_args(&["query", "--max-amount", "2000000", "http://example.com"]);
        let cli = Cli::try_parse_from(["pget"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_exceeded() {
        let query = make_query_args(&["query", "--max-amount", "500000", "http://example.com"]);
        let cli = Cli::try_parse_from(["pget"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
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
        let query = make_query_args(&["query", "--max-amount", "1000000", "http://example.com"]);
        let cli = Cli::try_parse_from(["pget"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_match() {
        let query = default_query();
        let cli = Cli::try_parse_from(["pget", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_no_match() {
        let query = default_query();
        let cli = Cli::try_parse_from(["pget", "--network", "ethereum"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
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
        let query = default_query();
        let cli = Cli::try_parse_from(["pget", "--network", "tempo-moderato, ethereum"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_tempo_network() {
        let query = default_query();
        let cli = Cli::try_parse_from(["pget", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_combined() {
        let query = make_query_args(&["query", "--max-amount", "2000000", "http://example.com"]);
        let cli = Cli::try_parse_from(["pget", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }
}
