//! Sign an MPP payment challenge and output the Authorization header value.

use std::io::{self, Read};

use anyhow::{Context as _, Result};
use mpp::client::PaymentProvider;
use mpp::protocol::methods::tempo::TempoChargeExt;

use tempo_common::cli::context::Context;
use tempo_common::cli::output::OutputFormat;
use tempo_common::error::{ConfigError, PaymentError};
use tempo_common::network::NetworkId;
use tempo_common::payment::classify::{classify_payment_error, map_mpp_validation_error};

/// Run the `sign` subcommand.
pub(crate) async fn run(ctx: &Context, challenge_arg: Option<String>, dry_run: bool) -> Result<()> {
    let raw = read_challenge(challenge_arg)?;
    let challenge =
        mpp::parse_www_authenticate(&raw).context("Failed to parse WWW-Authenticate challenge")?;

    // Enforce supported payment protocol
    if !challenge.method.eq_ignore_ascii_case("tempo") {
        return Err(PaymentError::UnsupportedPaymentMethod(challenge.method.to_string()).into());
    }

    // Resolve network from challenge
    let network = if let Ok(charge) = challenge.request.decode::<mpp::ChargeRequest>() {
        require_chain(charge.chain_id())?
    } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
        use mpp::protocol::methods::tempo::session::TempoSessionExt;
        require_chain(session.chain_id())?
    } else {
        return Err(PaymentError::InvalidChallenge(
            "unsupported payment challenge payload".to_string(),
        )
        .into());
    };

    // Enforce --network pin: reject challenges for a different chain
    if let Some(pinned) = ctx.requested_network {
        if pinned != network {
            return Err(PaymentError::InvalidChallenge(format!(
                "challenge network '{}' does not match --network '{}'",
                network.as_str(),
                pinned.as_str(),
            ))
            .into());
        }
    }

    // Validate challenge
    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    if dry_run {
        eprintln!("Challenge is valid.");
        return Ok(());
    }

    // Sign
    let signer = ctx.keys.signer(network)?;
    if signer.se_key_label.is_some() {
        anyhow::bail!("Sign command is not yet supported with Secure Enclave wallets.");
    }
    let from = signer.from;
    let rpc_url = ctx.config.rpc_url(network);

    let provider = mpp::client::TempoProvider::new(signer.signer.clone(), rpc_url.as_str())
        .map_err(|e| ConfigError::Invalid(e.to_string()))?
        .with_signing_mode(signer.signing_mode);

    let credential = provider
        .pay(&challenge)
        .await
        .map_err(|e| classify_payment_error(e, &network))?;

    let auth_header =
        mpp::format_authorization(&credential).context("Failed to format Authorization header")?;

    // Output
    match ctx.output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            let payload = serde_json::json!({
                "authorization": auth_header,
                "from": format!("{:#x}", from),
            });
            tempo_common::cli::output::emit_structured(ctx.output_format, &payload)?;
        }
        OutputFormat::Text => {
            println!("{auth_header}");
        }
    }

    Ok(())
}

/// Read the challenge from --challenge flag or stdin.
fn read_challenge(flag: Option<String>) -> Result<String> {
    if let Some(value) = flag {
        return Ok(value);
    }

    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("Failed to read challenge from stdin")?;
    let trimmed = buf.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("No challenge provided. Use --challenge or pipe via stdin.");
    }
    Ok(trimmed)
}

/// Resolve a chain ID to a known `NetworkId`.
fn require_chain(chain_id: Option<u64>) -> Result<NetworkId> {
    let cid = chain_id.ok_or_else(|| {
        PaymentError::InvalidChallenge("missing chainId in payment request".to_string())
    })?;
    NetworkId::from_chain_id(cid)
        .ok_or_else(|| PaymentError::InvalidChallenge(format!("unsupported chainId: {cid}")).into())
}
