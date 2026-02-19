//! Machine Payments Protocol (MPP) handling for the CLI
//!
//! This module handles the MPP protocol (https://mpp.sh) which uses
//! WWW-Authenticate and Authorization headers for HTTP-native payments.

use anyhow::{Context, Result};

use mpp::{parse_receipt, parse_www_authenticate, ChargeRequest, PaymentChallenge};

use crate::cli::formatting::format_address_link;
use crate::cli::hyperlink::hyperlink;
use crate::cli::Cli;
use crate::config::Config;
use crate::http::request::RequestContext;
use crate::http::HttpResponse;
use crate::network::explorer::ExplorerConfig;
use crate::payment::mpp_ext::{extract_tx_hash, network_from_charge_request, validate_challenge};
use crate::payment::provider::PrestoPaymentProvider;

/// Handle MPP charge flow (402 with WWW-Authenticate: Payment header)
pub async fn handle_charge_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    initial_response: &HttpResponse,
) -> Result<HttpResponse> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| crate::error::PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    let charge_req: ChargeRequest = challenge
        .request
        .decode()
        .context("Failed to parse charge request from challenge")?;
    let network_enum = network_from_charge_request(&charge_req)?;
    let explorer = network_enum.info().explorer;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Challenge ID: {}", challenge.id);
        eprintln!("Payment method: {}", challenge.method);
        eprintln!("Payment intent: {}", challenge.intent);
        if let Some(ref expires) = challenge.expires {
            eprintln!("Expires: {}", expires);
        }
    }

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

    validate_charge_constraints(&request_ctx.cli, &charge_req)?;

    if request_ctx.query.dry_run {
        return handle_web_dry_run(&challenge, &charge_req, explorer.as_ref());
    }

    // Use mpp::client::PaymentProvider to create the credential
    let provider = PrestoPaymentProvider::new(config.clone());

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Creating payment credential...");
    }

    use mpp::client::PaymentProvider;
    let credential = provider
        .pay(&challenge)
        .await
        .map_err(classify_payment_provider_error)?;

    let auth_header =
        mpp::format_authorization(&credential).context("Failed to format Authorization header")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
        eprintln!("Submitting payment to server...");
    }

    let payment_headers = vec![("Authorization".to_string(), auth_header)];
    let response = request_ctx.execute(url, Some(&payment_headers)).await?;

    if response.status_code >= 400 {
        return Err(parse_payment_rejection(&response).into());
    }

    let token_symbol = network_enum
        .token_config_by_address(&charge_req.currency)
        .map(|t| t.currency)
        .unwrap_or(crate::payment::currency::currencies::USDCE);
    display_web_receipt(
        &request_ctx.cli,
        &response,
        explorer.as_ref(),
        &charge_req.amount,
        &token_symbol,
    )?;

    Ok(response)
}

fn validate_charge_constraints(cli: &Cli, charge_req: &ChargeRequest) -> Result<()> {
    if let Some(ref networks) = cli.network {
        let allowed: Vec<&str> = networks.split(',').map(|s| s.trim()).collect();
        let network = network_from_charge_request(charge_req)?;
        let network_str = network.as_str();

        anyhow::ensure!(
            allowed.contains(&network_str),
            "Network '{}' not in allowed networks: {:?}",
            network_str,
            allowed
        );
    }

    Ok(())
}

/// Handle dry-run mode for web payments
fn handle_web_dry_run(
    challenge: &PaymentChallenge,
    charge_req: &ChargeRequest,
    explorer: Option<&ExplorerConfig>,
) -> Result<HttpResponse> {
    let network = network_from_charge_request(charge_req)?;

    let from_address = crate::wallet::credentials::WalletCredentials::load()
        .ok()
        .filter(|c| c.has_wallet())
        .map(|c| c.account_address)
        .unwrap_or_else(|| "unknown".to_string());

    println!("[DRY RUN] Web Payment would be made:");
    println!("Protocol: MPP (https://mpp.sh)");
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

    Ok(HttpResponse {
        status_code: 200,
        headers: std::collections::HashMap::new(),
        body: Vec::new(),
    })
}

/// Display receipt information from response with optional clickable explorer links.
fn display_web_receipt(
    cli: &Cli,
    response: &HttpResponse,
    explorer: Option<&ExplorerConfig>,
    amount: &str,
    currency: &crate::payment::currency::Currency,
) -> Result<()> {
    if let Some(receipt_header) = response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_header) {
            if cli.is_verbose() {
                let tx_ref = extract_tx_hash(receipt_header).unwrap_or(receipt.reference);

                // Format amount: "0.01 USDC"
                let amount_display = amount
                    .parse::<u128>()
                    .ok()
                    .map(|a| currency.format_trimmed(a))
                    .unwrap_or_else(|| format!("{} {}", amount, currency.symbol));

                let link = if let Some(exp) = explorer {
                    let url = exp.tx_url(&tx_ref);
                    hyperlink(&url, &url)
                } else {
                    tx_ref
                };
                eprintln!("Paid {amount_display} · {link}");
                eprintln!("  Status: {}", receipt.status);
                eprintln!("  Method: {}", receipt.method);
                eprintln!("  Timestamp: {}", receipt.timestamp);
            }
        }
    }
    Ok(())
}

/// Classify an mpp provider error into a PrestoError with actionable context.
fn classify_payment_provider_error(err: mpp::MppError) -> crate::error::PrestoError {
    let raw = err.to_string();
    let msg = raw.strip_prefix("HTTP error: ").unwrap_or(&raw).to_string();
    let msg_lower = msg.to_lowercase();

    if msg_lower.contains("not provisioned") {
        return crate::error::PrestoError::AccessKeyNotProvisioned;
    }

    if msg_lower.contains("spending limit exceeded") || msg_lower.contains("spending limit too low")
    {
        let token = extract_field(&msg, "need")
            .and_then(|s| s.split_whitespace().nth(1).map(|t| t.to_string()));
        let limit = extract_field(&msg, "limit is")
            .and_then(|s| s.split_whitespace().next().map(|v| v.to_string()));
        let required = extract_field(&msg, "need")
            .and_then(|s| s.split_whitespace().next().map(|v| v.to_string()));

        if let (Some(token), Some(limit), Some(required)) = (token, limit, required) {
            return crate::error::PrestoError::SpendingLimitExceeded {
                token,
                limit,
                required,
            };
        }
    }

    if msg_lower.contains("insufficient") && msg_lower.contains("balance") {
        let token = extract_between(&msg, "Insufficient ", " balance");
        let available = extract_field(&msg, "have")
            .and_then(|s| s.split_whitespace().next().map(|v| v.to_string()));
        let required = extract_field(&msg, "need")
            .and_then(|s| s.split_whitespace().next().map(|v| v.to_string()));

        if let (Some(token), Some(available), Some(required)) = (token, available, required) {
            return crate::error::PrestoError::InsufficientBalance {
                token,
                available,
                required,
            };
        }
    }

    crate::error::PrestoError::Http(msg)
}

/// Extract text between two markers, e.g. extract_between("Insufficient pathUSD balance", "Insufficient ", " balance") => "pathUSD".
fn extract_between(msg: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = msg.find(start)?;
    let after = &msg[start_idx + start.len()..];
    let end_idx = after.find(end)?;
    let value = after[..end_idx].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Extract a field value from error messages like "have X, need Y" or "limit is X".
fn extract_field(msg: &str, prefix: &str) -> Option<String> {
    let search = format!("{} ", prefix);
    let idx = msg.find(&search)?;
    let after = &msg[idx + search.len()..];
    let end = after.find([',', '\n']).unwrap_or(after.len());
    let value = after[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Parse a non-200 response after payment submission into a descriptive error.
fn parse_payment_rejection(response: &HttpResponse) -> crate::error::PrestoError {
    let reason = if let Ok(body) = response.body_string() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
                error.to_string()
            } else if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
                message.to_string()
            } else if let Some(detail) = json.get("detail").and_then(|d| d.as_str()) {
                detail.to_string()
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

    crate::error::PrestoError::PaymentRejected {
        reason,
        status_code: response.status_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use mpp::{MethodName, PaymentChallenge};

    fn mock_challenge(method: MethodName, amount: &str) -> (PaymentChallenge, ChargeRequest) {
        use mpp::Base64UrlJson;

        let charge_req = ChargeRequest {
            amount: amount.to_string(),
            currency: "0x20c0000000000000000000000000000000000001".to_string(),
            decimals: None,
            recipient: Some("0x1234567890123456789012345678901234567890".to_string()),
            expires: Some("2099-12-31T23:59:59Z".to_string()),
            description: None,
            external_id: None,
            method_details: Some(serde_json::json!({ "chainId": 42431 })),
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
        let cli = Cli::try_parse_from(["presto"]).unwrap();
        let (_challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_charge_constraints(&cli, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_match() {
        let cli = Cli::try_parse_from(["presto", "--network", "tempo-moderato"]).unwrap();
        let (_challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_charge_constraints(&cli, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_network_filter_no_match() {
        let cli = Cli::try_parse_from(["presto", "--network", "ethereum"]).unwrap();
        let (_challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_charge_constraints(&cli, &charge_req);
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
        let cli = Cli::try_parse_from(["presto", "--network", "tempo-moderato, ethereum"]).unwrap();
        let (_challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_charge_constraints(&cli, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_constraints_tempo_network() {
        let cli = Cli::try_parse_from(["presto", "--network", "tempo-moderato"]).unwrap();
        let (_challenge, charge_req) = mock_challenge(MethodName::new("tempo"), "1000000");

        let result = validate_charge_constraints(&cli, &charge_req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_field_with_decimals() {
        let msg = "limit is 0.000000 pathUSD, need 0.010000 pathUSD";
        assert_eq!(
            extract_field(msg, "limit is"),
            Some("0.000000 pathUSD".into())
        );
        assert_eq!(extract_field(msg, "need"), Some("0.010000 pathUSD".into()));
    }

    #[test]
    fn test_extract_field_no_match() {
        assert_eq!(extract_field("no match here", "limit is"), None);
    }

    #[test]
    fn test_extract_between_token() {
        let msg = "Insufficient pathUSD balance: have 0.50, need 1.00";
        assert_eq!(
            extract_between(msg, "Insufficient ", " balance"),
            Some("pathUSD".into())
        );
    }

    #[test]
    fn test_extract_between_address_token() {
        let msg = "Insufficient 0x20c0000000000000000000000000000000000000 balance: have 0, need 1";
        assert_eq!(
            extract_between(msg, "Insufficient ", " balance"),
            Some("0x20c0000000000000000000000000000000000000".into())
        );
    }

    #[test]
    fn test_extract_between_no_match() {
        assert_eq!(
            extract_between("no match", "Insufficient ", " balance"),
            None
        );
    }

    #[test]
    fn test_classify_spending_limit_from_mpp_error() {
        let inner = "Spending limit exceeded: limit is 0.000000 pathUSD, need 0.010000 pathUSD";
        let err = mpp::MppError::Http(inner.to_string());
        let result = classify_payment_provider_error(err);
        match result {
            crate::error::PrestoError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(limit, "0.000000");
                assert_eq!(required, "0.010000");
            }
            other => panic!("Expected SpendingLimitExceeded, got: {other}"),
        }
    }

    #[test]
    fn test_classify_spending_limit_with_address_token() {
        let addr = "0x20c0000000000000000000000000000000000000";
        let inner = format!("Spending limit exceeded: limit is 0.50 {addr}, need 1.00 {addr}");
        let err = mpp::MppError::Http(inner);
        let result = classify_payment_provider_error(err);
        match result {
            crate::error::PrestoError::SpendingLimitExceeded { token, .. } => {
                assert_eq!(token, addr);
            }
            other => panic!("Expected SpendingLimitExceeded, got: {other}"),
        }
    }

    #[test]
    fn test_classify_insufficient_balance_from_mpp_error() {
        let inner = "Insufficient pathUSD balance: have 0.50, need 1.00";
        let err = mpp::MppError::Http(inner.to_string());
        let result = classify_payment_provider_error(err);
        match result {
            crate::error::PrestoError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(available, "0.50");
                assert_eq!(required, "1.00");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_unrecognized_falls_through() {
        let err = mpp::MppError::Http("something unexpected".to_string());
        let result = classify_payment_provider_error(err);
        match result {
            crate::error::PrestoError::Http(msg) => {
                assert_eq!(msg, "something unexpected");
            }
            other => panic!("Expected Http passthrough, got: {other}"),
        }
    }
}
