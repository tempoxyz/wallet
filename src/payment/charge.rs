//! Machine Payments Protocol (MPP) handling for the CLI
//!
//! This module handles the MPP protocol (https://mpp.sh) which uses
//! WWW-Authenticate and Authorization headers for HTTP-native payments.

use anyhow::{Context, Result};

use mpp::{parse_receipt, parse_www_authenticate, ChargeRequest, PaymentChallenge};

use crate::cli::output::{format_address_link, hyperlink};
use crate::cli::Cli;
use crate::config::Config;
use crate::http::request::RequestContext;
use crate::http::HttpResponse;
use crate::network::ExplorerConfig;
use crate::error::{classify_payment_error, map_mpp_validation_error};
use crate::network::Network;
use mpp::protocol::core::extract_tx_hash;

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

    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    validate_charge_constraints(&request_ctx.cli, &charge_req)?;

    if request_ctx.query.dry_run {
        return handle_web_dry_run(&challenge, &charge_req, explorer.as_ref());
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Creating payment credential...");
    }

    // Build an mpp::client::TempoProvider with presto's config
    let provider = build_tempo_provider(config, network_enum)?;

    use mpp::client::PaymentProvider;
    let credential = provider
        .pay(&challenge)
        .await
        .map_err(classify_payment_error)?;

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

    let token = network_enum
        .token_config_by_address(&charge_req.currency);
    let symbol = token.map(|t| t.symbol).unwrap_or("USDC");
    let decimals = token.map(|t| t.decimals).unwrap_or(6);
    display_web_receipt(
        &request_ctx.cli,
        &response,
        explorer.as_ref(),
        &charge_req.amount,
        symbol,
        decimals,
    )?;

    Ok(response)
}

/// Build an mpp::client::TempoProvider from presto's config and network.
///
/// Constructs the mpp-rs payment provider with presto-specific configuration:
/// signer from wallet credentials, keychain signing mode, and stuck-tx
/// replacement. Nonce/gas resolution happens lazily at `.pay()` time inside
/// mpp-rs, not at provider construction time.
fn build_tempo_provider(
    config: &Config,
    network: crate::network::Network,
) -> Result<mpp::client::TempoProvider> {
    use crate::wallet::signer::load_wallet_signer;

    let network_name = network.as_str();
    let signing = load_wallet_signer(network_name)?;
    let network_info = config.resolve_network(network_name)?;

    let provider = mpp::client::TempoProvider::new(signing.signer.clone(), &network_info.rpc_url)
        .map_err(|e| crate::error::PrestoError::InvalidConfig(e.to_string()))?
        .with_signing_mode(signing.signing_mode)
        .with_replace_stuck_transactions(true);

    Ok(provider)
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
    symbol: &str,
    decimals: u8,
) -> Result<()> {
    if let Some(receipt_header) = response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_header) {
            if cli.is_verbose() {
                let tx_ref = extract_tx_hash(receipt_header).unwrap_or(receipt.reference);

                // Format amount: "0.01 USDC"
                let amount_display = amount
                    .parse::<u128>()
                    .ok()
                    .map(|a| format_token_amount(a, symbol, decimals))
                    .unwrap_or_else(|| format!("{} {}", amount, symbol));

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

/// Format atomic token units as a human-readable string with trimmed trailing zeros.
///
/// e.g., `format_token_amount(1_500_000, "USDC", 6)` → `"1.5 USDC"`
fn format_token_amount(atomic: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = atomic / divisor;
    let remainder = atomic % divisor;

    if remainder == 0 {
        format!("{whole} {symbol}")
    } else {
        let frac_str = format!("{:0width$}", remainder, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{whole}.{trimmed} {symbol}")
    }
}

/// Derive the network from a charge request's chain ID.
fn network_from_charge_request(req: &ChargeRequest) -> Result<Network> {
    use mpp::protocol::methods::tempo::TempoChargeExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in charge request".to_string())
    })?;
    Ok(Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })?)
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

}
