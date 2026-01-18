//! Inspect command for viewing payment requirements without executing payment

use anyhow::{Context, Result};
use purl_lib::{HttpClientBuilder, HttpMethod, PaymentMethod, PaymentRequirementsResponse};
use serde::Serialize;

use crate::cli::{Cli, OutputFormat};
use crate::config_utils::load_config;

/// Output format for a single payment option
#[derive(Debug, Serialize)]
struct AcceptedPaymentOption {
    network: String,
    scheme: String,
    amount_atomic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount_human: Option<String>,
    asset: String,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    decimals: Option<u8>,
    compatible: bool,
    pay_to: String,
    description: String,
}

/// Output format for the inspect command
#[derive(Debug, Serialize)]
struct InspectOutput {
    x402_version: u32,
    message: String,
    accepts: Vec<AcceptedPaymentOption>,
    configured_methods: Vec<String>,
}

/// Format atomic units to human-readable amounts
fn format_amount(atomic: &str, decimals: u8, symbol: &str) -> String {
    use purl_lib::currency::format_atomic_trimmed;
    format_atomic_trimmed(atomic, decimals, symbol)
}

/// Get token symbol for a requirement
fn get_token_symbol(requirement: &purl_lib::protocol::x402::PaymentRequirements) -> String {
    if let Some(extra) = requirement.extra() {
        if let Some(symbol) = extra.get("symbol").and_then(|s| s.as_str()) {
            return symbol.to_string();
        }
    }

    requirement.asset().to_string()
}

/// Get decimals for a token on a network
fn get_decimals(network: &str, asset: &str) -> Result<u8> {
    purl_lib::constants::get_token_decimals(network, asset).map_err(|e| {
        anyhow::anyhow!("Token decimals not configured: {e}. Add to ~/.purl/tokens.json if needed.")
    })
}

/// Inspect payment requirements for a URL
pub fn inspect_command(cli: &Cli, url: &str) -> Result<()> {
    let config = load_config(cli.config.as_ref())?;

    // Build HTTP client
    let mut builder = HttpClientBuilder::new()
        .verbose(cli.is_verbose())
        .follow_redirects(cli.follow_redirects);

    if let Some(timeout) = cli.get_timeout() {
        builder = builder.timeout(timeout);
    }

    if let Some(user_agent) = &cli.user_agent {
        builder = builder.user_agent(user_agent);
    }

    let mut client = builder.build()?;

    if cli.is_verbose() && cli.should_show_output() {
        eprintln!("Inspecting payment requirements for: {url}");
    }

    let response = client.request(HttpMethod::Get, url, None)?;

    if !response.is_payment_required() {
        anyhow::bail!(
            "No payment required (status: {}). URL does not require payment.",
            response.status_code
        );
    }

    let json = response.payment_requirements_json()?;
    let requirements: PaymentRequirementsResponse =
        serde_json::from_str(&json).context("Failed to parse payment requirements")?;

    let available_methods = config.available_payment_methods();

    match cli.effective_output_format() {
        OutputFormat::Json => {
            output_json(cli, &requirements, &available_methods)?;
        }
        OutputFormat::Yaml => {
            output_yaml(cli, &requirements, &available_methods)?;
        }
        OutputFormat::Text => {
            output_text(cli, &requirements, &available_methods)?;
        }
    }

    Ok(())
}

/// Build structured output from payment requirements
fn build_inspect_output(
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> InspectOutput {
    let accepts = requirements
        .accepts()
        .iter()
        .map(|req| {
            let symbol = get_token_symbol(req);
            let decimals = get_decimals(req.network(), req.asset()).ok();
            let amount_result = req.parse_max_amount();
            let amount_atomic = amount_result
                .as_ref()
                .map(|a| a.to_string())
                .unwrap_or_else(|_| "invalid".to_string());
            let amount_human = decimals.and_then(|dec| {
                amount_result
                    .ok()
                    .map(|amt| format_amount(&amt.to_string(), dec, &symbol))
            });

            AcceptedPaymentOption {
                network: req.network().to_string(),
                scheme: req.scheme().to_string(),
                amount_atomic,
                amount_human,
                asset: req.asset().to_string(),
                symbol,
                decimals,
                compatible: is_compatible_method(req, available_methods),
                pay_to: req.pay_to().to_string(),
                description: req.description().to_string(),
            }
        })
        .collect();

    InspectOutput {
        x402_version: requirements.version(),
        message: requirements
            .error()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        accepts,
        configured_methods: available_methods
            .iter()
            .map(|m| m.as_str().to_string())
            .collect(),
    }
}

fn output_json(
    cli: &Cli,
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_inspect_output(requirements, available_methods);
    let pretty_json = serde_json::to_string_pretty(&output)?;

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &pretty_json)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{pretty_json}");
    }

    Ok(())
}

fn output_yaml(
    cli: &Cli,
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_inspect_output(requirements, available_methods);
    let yaml_output = serde_yaml::to_string(&output)?;

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &yaml_output)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{yaml_output}");
    }

    Ok(())
}

fn output_text(
    cli: &Cli,
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let data = build_inspect_output(requirements, available_methods);
    let mut output = String::new();

    output.push_str("Payment Required (402)\n");
    output.push_str(&format!("Message: {}\n", data.message));
    output.push_str(&format!("x402 Version: {}\n", data.x402_version));
    output.push_str("\nAccepts:\n");

    for opt in &data.accepts {
        output.push_str(&format!("  - Network: {}\n", opt.network));
        output.push_str(&format!("    Scheme: {}\n", opt.scheme));

        if let Some(ref human) = opt.amount_human {
            output.push_str(&format!(
                "    Amount: {} ({} atomic units)\n",
                human, opt.amount_atomic
            ));
        } else {
            output.push_str(&format!(
                "    Amount: {} atomic units (decimals unknown)\n",
                opt.amount_atomic
            ));
        }

        output.push_str(&format!("    Asset: {}\n", opt.asset));
        output.push_str(&format!("    Pay To: {}\n", opt.pay_to));

        if !opt.description.is_empty() {
            output.push_str(&format!("    Description: {}\n", opt.description));
        }

        if opt.compatible {
            output.push_str("    Compatible: Yes (configured)\n");
        } else {
            output.push_str("    Compatible: No (not configured)\n");
        }

        output.push('\n');
    }

    output.push_str(&format!(
        "Configured payment methods: {}\n",
        if data.configured_methods.is_empty() {
            "None".to_string()
        } else {
            data.configured_methods.join(", ")
        }
    ));

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &output)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        print!("{output}");
    }

    Ok(())
}

/// Check if a requirement is compatible with configured payment methods
fn is_compatible_method(
    req: &purl_lib::protocol::x402::PaymentRequirements,
    available_methods: &[PaymentMethod],
) -> bool {
    if req.is_evm() && available_methods.contains(&PaymentMethod::Evm) {
        return true;
    }
    if req.is_solana() && available_methods.contains(&PaymentMethod::Solana) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use purl_lib::protocol::x402::{v1, PaymentRequirements};

    fn mock_evm_requirement() -> PaymentRequirements {
        let v1_req = v1::PaymentRequirements {
            scheme: "exact".to_string(),
            network: "base".to_string(),
            max_amount_required: "1000000".to_string(),
            resource: "https://example.com/resource".to_string(),
            description: "Test payment".to_string(),
            mime_type: "application/json".to_string(),
            output_schema: None,
            pay_to: "0x1234567890123456789012345678901234567890".to_string(),
            max_timeout_seconds: 300,
            asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            extra: Some(serde_json::json!({
                "name": "USD Coin",
                "version": "2",
                "symbol": "USDC"
            })),
        };
        PaymentRequirements::V1(v1_req)
    }

    fn mock_solana_requirement() -> PaymentRequirements {
        let v1_req = v1::PaymentRequirements {
            scheme: "exact".to_string(),
            network: "solana".to_string(),
            max_amount_required: "2000000".to_string(),
            resource: "https://example.com/resource".to_string(),
            description: "Solana payment".to_string(),
            mime_type: "application/json".to_string(),
            output_schema: None,
            pay_to: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            max_timeout_seconds: 300,
            asset: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            extra: None,
        };
        PaymentRequirements::V1(v1_req)
    }

    #[test]
    fn test_format_amount_eth() {
        // 1 ETH = 1000000000000000000 wei
        assert_eq!(format_amount("1000000000000000000", 18, "ETH"), "1 ETH");
        assert_eq!(format_amount("100000000000000000", 18, "ETH"), "0.1 ETH");
        assert_eq!(format_amount("10000000000000000", 18, "ETH"), "0.01 ETH");
        assert_eq!(format_amount("1000000000000000", 18, "ETH"), "0.001 ETH");
    }

    #[test]
    fn test_format_amount_usdc() {
        // USDC has 6 decimals
        assert_eq!(format_amount("1000000", 6, "USDC"), "1 USDC");
        assert_eq!(format_amount("100000", 6, "USDC"), "0.1 USDC");
        assert_eq!(format_amount("10000", 6, "USDC"), "0.01 USDC");
        assert_eq!(format_amount("1000", 6, "USDC"), "0.001 USDC");
    }

    #[test]
    fn test_format_amount_sol() {
        // SOL has 9 decimals (lamports)
        assert_eq!(format_amount("1000000000", 9, "SOL"), "1 SOL");
        assert_eq!(format_amount("100000000", 9, "SOL"), "0.1 SOL");
        assert_eq!(format_amount("10000000", 9, "SOL"), "0.01 SOL");
        assert_eq!(format_amount("1000000", 9, "SOL"), "0.001 SOL");
    }

    #[test]
    fn test_format_amount_no_fraction() {
        assert_eq!(format_amount("5000000000000000000", 18, "ETH"), "5 ETH");
        assert_eq!(format_amount("5000000", 6, "USDC"), "5 USDC");
    }

    #[test]
    fn test_format_amount_zero() {
        assert_eq!(format_amount("0", 18, "ETH"), "0 ETH");
        assert_eq!(format_amount("0", 6, "USDC"), "0 USDC");
    }

    #[test]
    fn test_get_token_symbol_from_extra() {
        let req = mock_evm_requirement();
        let symbol = get_token_symbol(&req);
        assert_eq!(symbol, "USDC");
    }

    #[test]
    fn test_get_token_symbol_fallback_to_asset() {
        let req = mock_solana_requirement();
        let symbol = get_token_symbol(&req);
        // Falls back to asset address when no symbol in extra
        assert_eq!(symbol, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    }

    #[test]
    fn test_is_compatible_method_evm_with_evm_config() {
        let req = mock_evm_requirement();
        let methods = vec![PaymentMethod::Evm];
        assert!(is_compatible_method(&req, &methods));
    }

    #[test]
    fn test_is_compatible_method_evm_without_config() {
        let req = mock_evm_requirement();
        let methods: Vec<PaymentMethod> = vec![];
        assert!(!is_compatible_method(&req, &methods));
    }

    #[test]
    fn test_is_compatible_method_solana_with_solana_config() {
        let req = mock_solana_requirement();
        let methods = vec![PaymentMethod::Solana];
        assert!(is_compatible_method(&req, &methods));
    }

    #[test]
    fn test_is_compatible_method_solana_with_wrong_config() {
        let req = mock_solana_requirement();
        let methods = vec![PaymentMethod::Evm];
        assert!(!is_compatible_method(&req, &methods));
    }

    #[test]
    fn test_is_compatible_method_with_both_configs() {
        let evm_req = mock_evm_requirement();
        let sol_req = mock_solana_requirement();
        let methods = vec![PaymentMethod::Evm, PaymentMethod::Solana];

        assert!(is_compatible_method(&evm_req, &methods));
        assert!(is_compatible_method(&sol_req, &methods));
    }

    #[test]
    fn test_build_inspect_output_structure() {
        use purl_lib::PaymentRequirementsResponse;

        // Create a v1 payment requirements response
        // Note: PaymentRequirementsResponse::V1 wraps v1::PaymentRequirementsResponse
        // which expects PaymentRequirements (the wrapper), not v1::PaymentRequirements
        let v1_response = v1::PaymentRequirementsResponse {
            x402_version: 1,
            error: "Payment required".to_string(),
            accepts: vec![v1::PaymentRequirements {
                scheme: "exact".to_string(),
                network: "base".to_string(),
                max_amount_required: "1000000".to_string(),
                resource: "https://example.com/resource".to_string(),
                description: "Test payment".to_string(),
                mime_type: "application/json".to_string(),
                output_schema: None,
                pay_to: "0x1234567890123456789012345678901234567890".to_string(),
                max_timeout_seconds: 300,
                asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                extra: Some(serde_json::json!({
                    "name": "USD Coin",
                    "version": "2",
                    "symbol": "USDC"
                })),
            }],
        };
        let response = PaymentRequirementsResponse::V1(v1_response);

        let available_methods = vec![PaymentMethod::Evm];
        let output = build_inspect_output(&response, &available_methods);

        assert_eq!(output.x402_version, 1);
        assert_eq!(output.message, "Payment required");
        assert_eq!(output.accepts.len(), 1);
        assert_eq!(output.configured_methods, vec!["evm".to_string()]);

        let option = &output.accepts[0];
        assert_eq!(option.network, "base");
        assert_eq!(option.scheme, "exact");
        assert_eq!(option.amount_atomic, "1000000");
        assert_eq!(option.symbol, "USDC");
        assert!(option.compatible);
        assert_eq!(option.pay_to, "0x1234567890123456789012345678901234567890");
    }

    #[test]
    fn test_build_inspect_output_no_compatible_methods() {
        use purl_lib::PaymentRequirementsResponse;

        let v1_response = v1::PaymentRequirementsResponse {
            x402_version: 1,
            error: "".to_string(),
            accepts: vec![v1::PaymentRequirements {
                scheme: "exact".to_string(),
                network: "base".to_string(),
                max_amount_required: "1000000".to_string(),
                resource: "https://example.com/resource".to_string(),
                description: "Test payment".to_string(),
                mime_type: "application/json".to_string(),
                output_schema: None,
                pay_to: "0x1234567890123456789012345678901234567890".to_string(),
                max_timeout_seconds: 300,
                asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                extra: None,
            }],
        };
        let response = PaymentRequirementsResponse::V1(v1_response);

        // Only Solana configured, but requirement is EVM
        let available_methods = vec![PaymentMethod::Solana];
        let output = build_inspect_output(&response, &available_methods);

        assert_eq!(output.accepts.len(), 1);
        assert!(!output.accepts[0].compatible);
    }

    #[test]
    fn test_build_inspect_output_multiple_accepts() {
        use purl_lib::PaymentRequirementsResponse;

        let v1_response = v1::PaymentRequirementsResponse {
            x402_version: 1,
            error: "".to_string(),
            accepts: vec![
                v1::PaymentRequirements {
                    scheme: "exact".to_string(),
                    network: "base".to_string(),
                    max_amount_required: "1000000".to_string(),
                    resource: "https://example.com/resource".to_string(),
                    description: "Test payment".to_string(),
                    mime_type: "application/json".to_string(),
                    output_schema: None,
                    pay_to: "0x1234567890123456789012345678901234567890".to_string(),
                    max_timeout_seconds: 300,
                    asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                    extra: None,
                },
                v1::PaymentRequirements {
                    scheme: "exact".to_string(),
                    network: "solana".to_string(),
                    max_amount_required: "2000000".to_string(),
                    resource: "https://example.com/resource".to_string(),
                    description: "Solana payment".to_string(),
                    mime_type: "application/json".to_string(),
                    output_schema: None,
                    pay_to: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                    max_timeout_seconds: 300,
                    asset: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                    extra: None,
                },
            ],
        };
        let response = PaymentRequirementsResponse::V1(v1_response);

        let available_methods = vec![PaymentMethod::Evm, PaymentMethod::Solana];
        let output = build_inspect_output(&response, &available_methods);

        assert_eq!(output.accepts.len(), 2);
        assert!(output.accepts[0].compatible); // EVM
        assert!(output.accepts[1].compatible); // Solana
    }

    #[test]
    fn test_build_inspect_output_empty_accepts() {
        use purl_lib::PaymentRequirementsResponse;

        let v1_response = v1::PaymentRequirementsResponse {
            x402_version: 1,
            error: "No payment options available".to_string(),
            accepts: vec![],
        };
        let response = PaymentRequirementsResponse::V1(v1_response);

        let available_methods = vec![PaymentMethod::Evm];
        let output = build_inspect_output(&response, &available_methods);

        assert!(output.accepts.is_empty());
        assert_eq!(output.message, "No payment options available");
    }

    #[test]
    fn test_accepted_payment_option_serialization() {
        let option = AcceptedPaymentOption {
            network: "base".to_string(),
            scheme: "exact".to_string(),
            amount_atomic: "1000000".to_string(),
            amount_human: Some("1 USDC".to_string()),
            asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            symbol: "USDC".to_string(),
            decimals: Some(6),
            compatible: true,
            pay_to: "0x1234".to_string(),
            description: "Test payment".to_string(),
        };

        let json = serde_json::to_value(&option).unwrap();

        assert_eq!(json["network"], "base");
        assert_eq!(json["scheme"], "exact");
        assert_eq!(json["amount_atomic"], "1000000");
        assert_eq!(json["amount_human"], "1 USDC");
        assert_eq!(json["symbol"], "USDC");
        assert_eq!(json["decimals"], 6);
        assert_eq!(json["compatible"], true);
    }

    #[test]
    fn test_accepted_payment_option_skip_serializing_none() {
        let option = AcceptedPaymentOption {
            network: "base".to_string(),
            scheme: "exact".to_string(),
            amount_atomic: "1000000".to_string(),
            amount_human: None, // Should be skipped
            asset: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            symbol: "USDC".to_string(),
            decimals: None, // Should be skipped
            compatible: false,
            pay_to: "0x1234".to_string(),
            description: "".to_string(),
        };

        let json = serde_json::to_string(&option).unwrap();

        // None fields should be skipped
        assert!(!json.contains("amount_human"));
        assert!(!json.contains("decimals"));
    }

    #[test]
    fn test_inspect_output_serialization() {
        let output = InspectOutput {
            x402_version: 1,
            message: "Payment required".to_string(),
            accepts: vec![],
            configured_methods: vec!["evm".to_string()],
        };

        let json = serde_json::to_value(&output).unwrap();

        assert_eq!(json["x402_version"], 1);
        assert_eq!(json["message"], "Payment required");
        assert!(json["accepts"].as_array().unwrap().is_empty());
        assert_eq!(json["configured_methods"][0], "evm");
    }
}
