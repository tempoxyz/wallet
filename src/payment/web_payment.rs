//! Web Payment Auth protocol handling for the CLI
//!
//! This module handles the IETF Web Payment Auth protocol (draft-ietf-httpauth-payment-01)
//! which uses WWW-Authenticate and Authorization headers for blockchain payments.

use anyhow::{Context, Result};
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
use crate::payment::provider::TempoCtlPaymentProvider;

/// Handle Web Payment Auth protocol (402 with WWW-Authenticate: Payment header)
pub async fn handle_web_payment_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    initial_response: &HttpResponse,
) -> Result<HttpResponse> {
    if let Ok(mode) = std::env::var("TEMPOCTL_MOCK_PAYMENT") {
        return handle_mock_payment(request_ctx, url, &mode).await;
    }

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
    let provider = TempoCtlPaymentProvider::with_no_swap(config.clone(), request_ctx.query.no_swap);

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Creating payment credential...");
    }

    use mpay::client::PaymentProvider;
    let credential = provider
        .pay(&challenge)
        .await
        .map_err(classify_payment_provider_error)?;

    let auth_header =
        mpay::format_authorization(&credential).context("Failed to format Authorization header")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
        eprintln!("Submitting payment to server...");
    }

    let payment_headers = vec![("Authorization".to_string(), auth_header)];
    let response = request_ctx.execute(url, Some(&payment_headers)).await?;

    if response.status_code >= 400 {
        return Err(parse_payment_rejection(&response).into());
    }

    display_web_receipt(&request_ctx.cli, &response, explorer.as_ref())?;

    Ok(response)
}

/// Handle mock payment mode for integration testing.
///
/// This enables testing of error display, suggestions, and exit codes
/// without requiring real wallet signing or RPC calls.
async fn handle_mock_payment(
    request_ctx: &RequestContext,
    url: &str,
    mode: &str,
) -> Result<HttpResponse> {
    match mode {
        "spending_limit_exceeded" => Err(crate::error::TempoCtlError::SpendingLimitExceeded {
            token: "pathUSD".into(),
            limit: "0.50".into(),
            required: "1.00".into(),
        }
        .into()),
        "insufficient_balance" => Err(crate::error::TempoCtlError::InsufficientBalance {
            token: "pathUSD".into(),
            available: "0.50".into(),
            required: "1.00".into(),
        }
        .into()),
        "payment_rejected" => {
            let payment_headers = vec![(
                "Authorization".to_string(),
                "Payment mock-credential".to_string(),
            )];
            let response = request_ctx.execute(url, Some(&payment_headers)).await?;
            if response.status_code >= 400 {
                return Err(parse_payment_rejection(&response).into());
            }
            Ok(response)
        }
        _ => anyhow::bail!("Unknown mock payment mode: {}", mode),
    }
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

/// Classify an mpay provider error into a TempoCtlError with actionable context.
fn classify_payment_provider_error(err: mpay::MppError) -> crate::error::TempoCtlError {
    let msg = err.to_string();
    let msg_lower = msg.to_lowercase();

    if msg_lower.contains("spending limit exceeded") || msg_lower.contains("spending limit too low")
    {
        return crate::error::TempoCtlError::SpendingLimitExceeded {
            token: extract_field(&msg, "need")
                .and_then(|s| s.split_whitespace().nth(1).map(|t| t.to_string()))
                .unwrap_or_else(|| "token".to_string()),
            limit: extract_field(&msg, "limit is")
                .and_then(|s| s.split_whitespace().next().map(|v| v.to_string()))
                .unwrap_or_else(|| "unknown".to_string()),
            required: extract_field(&msg, "need")
                .and_then(|s| s.split_whitespace().next().map(|v| v.to_string()))
                .unwrap_or_else(|| "unknown".to_string()),
        };
    }

    if msg_lower.contains("insufficient") && msg_lower.contains("balance") {
        return crate::error::TempoCtlError::InsufficientBalance {
            token: extract_field(&msg, "token").unwrap_or_else(|| "token".to_string()),
            available: extract_field(&msg, "have").unwrap_or_else(|| "unknown".to_string()),
            required: extract_field(&msg, "need").unwrap_or_else(|| "unknown".to_string()),
        };
    }

    crate::error::TempoCtlError::Http(format!("Failed to create payment credential: {}", msg))
}

/// Extract a field value from error messages like "have X, need Y" or "Insufficient TOKEN balance: have X, need Y".
fn extract_field(msg: &str, prefix: &str) -> Option<String> {
    let search = format!("{} ", prefix);
    let idx = msg.find(&search)?;
    let after = &msg[idx + search.len()..];
    let end = after.find([',', '.', '\n']).unwrap_or(after.len());
    let value = after[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Parse a non-200 response after payment submission into a descriptive error.
fn parse_payment_rejection(response: &HttpResponse) -> crate::error::TempoCtlError {
    let reason = if let Ok(body) = response.body_string() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
                error.to_string()
            } else if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
                message.to_string()
            } else {
                format!("HTTP {}", response.status_code)
            }
        } else if !body.trim().is_empty() {
            body.chars().take(200).collect()
        } else {
            format!("HTTP {}", response.status_code)
        }
    } else {
        format!("HTTP {}", response.status_code)
    };

    crate::error::TempoCtlError::PaymentRejected {
        reason,
        status_code: response.status_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_utils::make_query_args;
    use clap::Parser;
    use mpay::{MethodName, PaymentChallenge};

    fn mock_challenge(method: MethodName, amount: &str) -> (PaymentChallenge, ChargeRequest) {
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

    fn default_query() -> QueryArgs {
        make_query_args(&["query", "http://example.com"])
    }

    #[test]
    fn test_validate_constraints_no_constraints() {
        let query = default_query();
        let cli = Cli::try_parse_from(["tempoctl"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_ok() {
        let query = make_query_args(&["query", "--max-amount", "2000000", "http://example.com"]);
        let cli = Cli::try_parse_from(["tempoctl"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_max_amount_exceeded() {
        let query = make_query_args(&["query", "--max-amount", "500000", "http://example.com"]);
        let cli = Cli::try_parse_from(["tempoctl"]).unwrap();
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
        let cli = Cli::try_parse_from(["tempoctl"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_match() {
        let query = default_query();
        let cli = Cli::try_parse_from(["tempoctl", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_no_match() {
        let query = default_query();
        let cli = Cli::try_parse_from(["tempoctl", "--network", "ethereum"]).unwrap();
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
        let cli =
            Cli::try_parse_from(["tempoctl", "--network", "tempo-moderato, ethereum"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_tempo_network() {
        let query = default_query();
        let cli = Cli::try_parse_from(["tempoctl", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_combined() {
        let query = make_query_args(&["query", "--max-amount", "2000000", "http://example.com"]);
        let cli = Cli::try_parse_from(["tempoctl", "--network", "tempo-moderato"]).unwrap();
        let (challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_web_payment_constraints(&query, &cli, &challenge, &charge_req);
        assert!(result.is_ok());
    }
}
