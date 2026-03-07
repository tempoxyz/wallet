//! Payment analytics tracking for query command flows.

use crate::analytics;
use crate::cli::Context;
use crate::util::sanitize_error;

use super::challenge::ChallengeContext;

// ---------------------------------------------------------------------------
// Pre-402 query tracking (no payment context needed)
// ---------------------------------------------------------------------------

pub(super) fn track_query_started(ctx: &Context, url: &str, method: &str) {
    ctx.track(
        analytics::Event::QueryStarted,
        analytics::QueryStartedPayload {
            url: url.to_string(),
            method: method.to_string(),
        },
    );
}

pub(super) fn track_query_failure(ctx: &Context, url: &str, method: &str, error: &str) {
    ctx.track(
        analytics::Event::QueryFailure,
        analytics::QueryFailurePayload {
            url: url.to_string(),
            method: method.to_string(),
            error: sanitize_error(error),
        },
    );
}

pub(super) fn track_query_success(ctx: &Context, url: &str, method: &str, status_code: u16) {
    ctx.track(
        analytics::Event::QuerySuccess,
        analytics::QuerySuccessPayload {
            url: url.to_string(),
            method: method.to_string(),
            status_code,
        },
    );
}

// ---------------------------------------------------------------------------
// Post-402 payment tracking
// ---------------------------------------------------------------------------

/// Helper for tracking payment analytics without duplication.
///
/// Created once after parsing the 402 challenge, then used to track
/// started/success/failure events identically for both charge and session flows.
pub(super) struct PaymentAnalytics<'a> {
    ctx: &'a Context,
    network: String,
    amount: String,
    currency: String,
    intent: String,
}

impl<'a> PaymentAnalytics<'a> {
    pub(super) fn from_challenge(challenge: &ChallengeContext, ctx: &'a Context) -> Self {
        Self {
            ctx,
            network: challenge.network.as_str().to_string(),
            amount: challenge.amount.clone(),
            currency: challenge.currency.clone(),
            intent: challenge.intent_str().to_string(),
        }
    }

    pub(super) fn track_started(&self) {
        self.ctx.track(
            analytics::Event::PaymentStarted,
            analytics::PaymentStartedPayload {
                network: self.network.clone(),
                amount: self.amount.clone(),
                currency: self.currency.clone(),
                intent: self.intent.clone(),
            },
        );
    }

    pub(super) fn track_success(
        &self,
        tx_hash: String,
        session_id: Option<String>,
        url: &str,
        method: &str,
        status_code: u16,
    ) {
        self.ctx.track(
            analytics::Event::PaymentSuccess,
            analytics::PaymentSuccessPayload {
                network: self.network.clone(),
                amount: self.amount.clone(),
                currency: self.currency.clone(),
                intent: self.intent.clone(),
                tx_hash,
                session_id,
            },
        );
        track_query_success(self.ctx, url, method, status_code);
    }

    pub(super) fn track_failure(&self, err: &anyhow::Error) {
        self.ctx.track(
            analytics::Event::PaymentFailure,
            analytics::PaymentFailurePayload {
                network: self.network.clone(),
                amount: self.amount.clone(),
                currency: self.currency.clone(),
                intent: self.intent.clone(),
                error: sanitize_error(&err.to_string()),
            },
        );
    }
}
