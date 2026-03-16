//! Sign an MPP payment challenge and output the Authorization header value.

use std::io::{self, Read};

use mpp::{client::PaymentProvider, protocol::methods::tempo::TempoChargeExt};

use tempo_common::{
    cli::{context::Context, output::OutputFormat},
    error::{ConfigError, InputError, PaymentError, TempoError},
    network::NetworkId,
    payment::{classify_payment_error, map_mpp_validation_error},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedPaymentMethod {
    Tempo,
}

impl SupportedPaymentMethod {
    fn parse(value: &str) -> Option<Self> {
        value.eq_ignore_ascii_case("tempo").then_some(Self::Tempo)
    }
}

/// Run the `sign` subcommand.
pub(crate) async fn run(
    ctx: &Context,
    challenge_arg: Option<String>,
    dry_run: bool,
) -> Result<(), TempoError> {
    let raw = read_challenge(challenge_arg)?;
    let challenge =
        mpp::parse_www_authenticate(&raw).map_err(|source| PaymentError::ChallengeParseSource {
            context: "WWW-Authenticate challenge",
            source: Box::new(source),
        })?;

    // Enforce supported payment protocol
    let method = challenge.method.to_string();
    if SupportedPaymentMethod::parse(&method).is_none() {
        return Err(PaymentError::UnsupportedPaymentMethod(challenge.method.to_string()).into());
    }

    // Resolve network from challenge
    let network = if let Ok(charge) = challenge.request.decode::<mpp::ChargeRequest>() {
        require_chain(charge.chain_id())?
    } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
        use mpp::protocol::methods::tempo::session::TempoSessionExt;
        require_chain(session.chain_id())?
    } else {
        return Err(PaymentError::ChallengeUnsupportedPayload {
            context: "payment challenge payload",
        }
        .into());
    };

    // Enforce --network pin: reject challenges for a different chain
    if let Some(pinned) = ctx.requested_network {
        if pinned != network {
            return Err(PaymentError::ChallengeNetworkMismatch {
                context: "payment challenge network",
                challenge_network: network.as_str().to_string(),
                configured_network: pinned.as_str().to_string(),
            }
            .into());
        }
    }

    // Validate challenge
    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    if dry_run {
        let payload = serde_json::json!({
            "valid": true,
            "method": challenge.method,
            "intent": challenge.intent.as_str(),
        });
        tempo_common::cli::output::emit_by_format(ctx.output_format, &payload, || {
            eprintln!("Challenge is valid.");
            Ok(())
        })?;
        return Ok(());
    }

    // Sign
    let signer = ctx.keys.signer(network)?;
    let from = signer.from;
    let rpc_url = ctx.config.rpc_url(network);

    let provider = mpp::client::TempoProvider::new(signer.signer.clone(), rpc_url.as_str())
        .map_err(|source| ConfigError::ProviderInitSource {
            provider: "tempo payment provider",
            source: Box::new(source),
        })?
        .with_signing_mode(signer.signing_mode);

    let credential = provider
        .pay(&challenge)
        .await
        .map_err(|e| classify_payment_error(e, &network))?;

    let auth_header = mpp::format_authorization(&credential).map_err(|source| {
        PaymentError::ChallengeFormatSource {
            context: "Authorization header",
            source: Box::new(source),
        }
    })?;

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
fn read_challenge(flag: Option<String>) -> Result<String, TempoError> {
    if let Some(value) = flag {
        return Ok(value);
    }

    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(InputError::ReadStdin)?;
    let trimmed = buf.trim().to_string();
    if trimmed.is_empty() {
        return Err(InputError::MissingChallenge.into());
    }
    Ok(trimmed)
}

/// Resolve a chain ID to a known `NetworkId`.
fn require_chain(chain_id: Option<u64>) -> Result<NetworkId, TempoError> {
    let cid = chain_id.ok_or(PaymentError::ChallengeMissingField {
        context: "payment request",
        field: "chainId",
    })?;
    NetworkId::from_chain_id(cid).ok_or_else(|| {
        PaymentError::ChallengeUnsupportedChainId {
            context: "payment request",
            chain_id: cid,
        }
        .into()
    })
}
