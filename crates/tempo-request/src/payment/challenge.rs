//! Shared payment-challenge parsing helpers.

use mpp::protocol::methods::tempo::session::TempoSessionExt;

use tempo_common::error::{PaymentError, TempoError};

/// Decode a session request from challenge payload without applying compatibility defaults.
pub(crate) fn decode_session_request(
    challenge: &mpp::PaymentChallenge,
) -> Result<mpp::SessionRequest, TempoError> {
    challenge
        .request
        .decode::<mpp::SessionRequest>()
        .map_err(|source| {
            PaymentError::ChallengeParseSource {
                context: "session request payload",
                source: Box::new(source),
            }
            .into()
        })
}

/// Require a chain ID in session method details.
pub(crate) fn require_session_chain_id(
    request: &mpp::SessionRequest,
    context: &'static str,
) -> Result<u64, TempoError> {
    request.chain_id().ok_or_else(|| {
        PaymentError::ChallengeMissingField {
            context,
            field: "chainId",
        }
        .into()
    })
}
