//! Inspect command for viewing payment requirements without executing payment

use mpay::{parse_www_authenticate, ChargeRequest, PaymentChallenge};

use crate::payment::mpay_ext::ChargeRequestExt;
use crate::{config::PaymentMethod, http::HttpClientBuilder, http::HttpMethod};
use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::{Cli, OutputFormat};
use crate::config::load_config;

/// Output format for the inspect command
#[derive(Debug, Serialize)]
struct InspectOutput {
    message: String,
    challenge: ChallengeInfo,
    configured_methods: Vec<String>,
}

/// Challenge information for output
#[derive(Debug, Serialize)]
struct ChallengeInfo {
    id: String,
    realm: String,
    method: String,
    intent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    charge: Option<ChargeInfo>,
    compatible: bool,
}

/// Charge request information for output
#[derive(Debug, Serialize)]
struct ChargeInfo {
    amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount_human: Option<String>,
    asset: String,
    destination: String,
    expires: String,
}

/// Inspect payment requirements for a URL
pub async fn inspect_command(cli: &Cli, url: &str) -> Result<()> {
    let config = load_config(cli.config.as_ref())?;

    let mut builder = HttpClientBuilder::new()
        .verbose(cli.is_verbose())
        .follow_redirects(cli.follow_redirects);

    if let Some(timeout) = cli.get_timeout() {
        builder = builder.timeout(timeout);
    }

    if let Some(user_agent) = &cli.user_agent {
        builder = builder.user_agent(user_agent);
    }

    let client = builder.build()?;

    if cli.is_verbose() && cli.should_show_output() {
        eprintln!("Inspecting payment requirements for: {url}");
    }

    let response = client.request(HttpMethod::Get, url, None).await?;

    if !response.is_payment_required() {
        anyhow::bail!(
            "No payment required (status: {}). URL does not require payment.",
            response.status_code
        );
    }

    let www_auth = response
        .get_header("www-authenticate")
        .ok_or_else(|| anyhow::anyhow!("Missing WWW-Authenticate header in 402 response"))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    let available_methods = config.available_payment_methods();

    match cli.effective_output_format() {
        OutputFormat::Json => {
            output_json(cli, &challenge, &available_methods)?;
        }
        OutputFormat::Yaml => {
            output_yaml(cli, &challenge, &available_methods)?;
        }
        OutputFormat::Text => {
            output_text(cli, &challenge, &available_methods)?;
        }
    }

    Ok(())
}

/// Build structured output from payment challenge
fn build_inspect_output(
    challenge: &PaymentChallenge,
    available_methods: &[PaymentMethod],
) -> InspectOutput {
    let charge_info = challenge
        .request
        .decode::<ChargeRequest>()
        .ok()
        .map(|req| ChargeInfo {
            amount: req.amount.clone(),
            amount_human: format_charge_amount(&req, challenge),
            asset: req.currency.clone(),
            destination: req.recipient.clone().unwrap_or_default(),
            expires: req.expires.clone().unwrap_or_default(),
        });

    let compatible = is_compatible_method(challenge, available_methods);

    InspectOutput {
        message: challenge.description.clone().unwrap_or_default(),
        challenge: ChallengeInfo {
            id: challenge.id.clone(),
            realm: challenge.realm.clone(),
            method: challenge.method.to_string(),
            intent: challenge.intent.to_string(),
            description: challenge.description.clone(),
            expires: challenge.expires.clone(),
            charge: charge_info,
            compatible,
        },
        configured_methods: available_methods
            .iter()
            .map(|m| m.as_str().to_string())
            .collect(),
    }
}

fn format_charge_amount(req: &ChargeRequest, challenge: &PaymentChallenge) -> Option<String> {
    use crate::network::Network;
    use crate::payment::mpay_ext::method_to_network;
    use std::str::FromStr;

    let network_name = method_to_network(&challenge.method)?;
    let network = Network::from_str(network_name).ok()?;

    let money = req.money(network).ok()?;
    Some(money.format_trimmed())
}

fn output_json(
    cli: &Cli,
    challenge: &PaymentChallenge,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_inspect_output(challenge, available_methods);
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
    challenge: &PaymentChallenge,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_inspect_output(challenge, available_methods);
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
    challenge: &PaymentChallenge,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let data = build_inspect_output(challenge, available_methods);
    let mut output = String::new();

    output.push_str("Payment Required (402)\n");
    if !data.message.is_empty() {
        output.push_str(&format!("Message: {}\n", data.message));
    }
    output.push_str("\nChallenge:\n");
    output.push_str(&format!("  ID: {}\n", data.challenge.id));
    output.push_str(&format!("  Realm: {}\n", data.challenge.realm));
    output.push_str(&format!("  Method: {}\n", data.challenge.method));
    output.push_str(&format!("  Intent: {}\n", data.challenge.intent));

    if let Some(ref expires) = data.challenge.expires {
        output.push_str(&format!("  Expires: {}\n", expires));
    }

    if let Some(ref charge) = data.challenge.charge {
        output.push_str("\n  Charge Request:\n");
        if let Some(ref human) = charge.amount_human {
            output.push_str(&format!(
                "    Amount: {} ({} atomic units)\n",
                human, charge.amount
            ));
        } else {
            output.push_str(&format!("    Amount: {} atomic units\n", charge.amount));
        }
        output.push_str(&format!("    Asset: {}\n", charge.asset));
        output.push_str(&format!("    Destination: {}\n", charge.destination));
        output.push_str(&format!("    Expires: {}\n", charge.expires));
    }

    if data.challenge.compatible {
        output.push_str("\n  Compatible: Yes (configured)\n");
    } else {
        output.push_str("\n  Compatible: No (not configured)\n");
    }

    output.push_str(&format!(
        "\nConfigured payment methods: {}\n",
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

/// Check if a challenge is compatible with configured payment methods
fn is_compatible_method(challenge: &PaymentChallenge, available_methods: &[PaymentMethod]) -> bool {
    use crate::payment::mpay_ext::method_to_network;

    // If the method maps to a known network, it requires EVM
    if method_to_network(&challenge.method).is_some() {
        available_methods.contains(&PaymentMethod::Evm)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpay::{Base64UrlJson, IntentName, MethodName};

    fn mock_challenge() -> PaymentChallenge {
        let charge_req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            recipient: Some("0x1234567890123456789012345678901234567890".to_string()),
            expires: Some("2099-12-31T23:59:59Z".to_string()),
            description: None,
            external_id: None,
            method_details: None,
        };

        PaymentChallenge {
            id: "test-challenge-id".to_string(),
            realm: "api.example.com".to_string(),
            method: MethodName::new("tempo"),
            intent: IntentName::new("charge"),
            request: Base64UrlJson::from_typed(&charge_req).unwrap(),
            digest: None,
            description: Some("Test payment".to_string()),
            expires: Some("2099-12-31T23:59:59Z".to_string()),
        }
    }

    #[test]
    fn test_is_compatible_method_with_evm_config() {
        let challenge = mock_challenge();
        let methods = vec![PaymentMethod::Evm];
        assert!(is_compatible_method(&challenge, &methods));
    }

    #[test]
    fn test_is_compatible_method_without_config() {
        let challenge = mock_challenge();
        let methods: Vec<PaymentMethod> = vec![];
        assert!(!is_compatible_method(&challenge, &methods));
    }

    #[test]
    fn test_build_inspect_output_structure() {
        let challenge = mock_challenge();
        let available_methods = vec![PaymentMethod::Evm];
        let output = build_inspect_output(&challenge, &available_methods);

        assert_eq!(output.message, "Test payment");
        assert_eq!(output.challenge.id, "test-challenge-id");
        assert_eq!(output.challenge.method, "tempo");
        assert_eq!(output.challenge.intent, "charge");
        assert!(output.challenge.compatible);
        assert!(output.challenge.charge.is_some());

        let charge = output.challenge.charge.unwrap();
        assert_eq!(charge.amount, "1000000");
        assert_eq!(
            charge.destination,
            "0x1234567890123456789012345678901234567890"
        );
    }

    #[test]
    fn test_inspect_output_serialization() {
        let challenge = mock_challenge();
        let available_methods = vec![PaymentMethod::Evm];
        let output = build_inspect_output(&challenge, &available_methods);

        let json = serde_json::to_value(&output).unwrap();

        assert_eq!(json["message"], "Test payment");
        assert_eq!(json["challenge"]["id"], "test-challenge-id");
        assert_eq!(json["challenge"]["method"], "tempo");
        assert_eq!(json["configured_methods"][0], "evm");
    }
}
