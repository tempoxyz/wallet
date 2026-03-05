//! Parsing and validation of 402 payment challenges.

use anyhow::{Context, Result};
use mpp::protocol::methods::tempo::session::TempoSessionExt;
use mpp::protocol::methods::tempo::TempoChargeExt;
use mpp::PaymentProtocol;

use crate::error::PrestoError;
use crate::http::HttpResponse;
use crate::keys::Keystore;
use crate::network::NetworkId;

/// Parsed payment challenge context extracted from a 402 response.
pub(super) struct ChallengeContext {
    pub(super) is_session: bool,
    pub(super) network: NetworkId,
    pub(super) amount: String,
    pub(super) currency: String,
    pub(super) challenge: mpp::PaymentChallenge,
}

/// Parse the WWW-Authenticate header from a 402 response and extract all
/// payment-related context needed for routing and analytics.
pub(super) fn parse_payment_challenge(response: &HttpResponse) -> Result<ChallengeContext> {
    let www_auth = response
        .header("www-authenticate")
        .ok_or_else(|| PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let _protocol = PaymentProtocol::detect(Some(www_auth))
        .ok_or_else(|| PrestoError::MissingHeader("WWW-Authenticate: Payment".to_string()))?;

    let challenge =
        mpp::parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    // Enforce supported payment protocol (tempo only for now)
    if !challenge.method.eq_ignore_ascii_case("tempo") {
        return Err(PrestoError::UnsupportedPaymentMethod(challenge.method.to_string()).into());
    }

    let is_session = challenge.intent.is_session();

    let require_chain = |chain_id: Option<u64>| -> Result<NetworkId> {
        let cid = chain_id.ok_or_else(|| {
            PrestoError::InvalidChallenge("missing chainId in payment request".to_string())
        })?;
        Ok(NetworkId::from_chain_id(cid)
            .ok_or_else(|| PrestoError::InvalidChallenge(format!("unsupported chainId: {cid}")))?)
    };

    let (network, amount, currency) =
        if let Ok(charge) = challenge.request.decode::<mpp::ChargeRequest>() {
            (
                require_chain(charge.chain_id())?,
                charge.amount,
                charge.currency,
            )
        } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
            (
                require_chain(session.chain_id())?,
                session.amount,
                session.currency,
            )
        } else {
            return Err(PrestoError::InvalidChallenge(
                "unsupported payment challenge payload".to_string(),
            )
            .into());
        };

    Ok(ChallengeContext {
        is_session,
        network,
        amount,
        currency,
        challenge,
    })
}

/// Ensure a wallet with a key for the challenge network is available.
pub(super) fn ensure_wallet_configured(
    keys: &Keystore,
    challenge_network: NetworkId,
) -> Result<()> {
    let chain_id = Some(challenge_network.chain_id());

    let setup_cmd = "presto login";

    if !keys.has_wallet() {
        anyhow::bail!(PrestoError::ConfigMissing(format!(
            "No wallet configured. Run '{setup_cmd}'."
        )));
    }

    if let Some(cid) = chain_id {
        let has_key = keys
            .keys
            .iter()
            .any(|k| k.chain_id == cid || k.chain_id == 0);
        if !has_key {
            anyhow::bail!(PrestoError::ConfigMissing(format!(
                "No key configured for network '{}'. Run '{setup_cmd}'.",
                challenge_network.as_str()
            )));
        }
    }

    Ok(())
}
