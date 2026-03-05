//! Payment analytics tracking for query command flows.

use crate::analytics::{self, Analytics};
use crate::util::sanitize_error;

use super::challenge::ChallengeContext;

/// Helper for tracking payment analytics without duplication.
///
/// Created once after parsing the 402 challenge, then used to track
/// started/success/failure events identically for both charge and session flows.
pub(super) struct PaymentAnalytics {
    analytics: Option<Analytics>,
    network: String,
    amount: String,
    currency: String,
    intent: String,
}

impl PaymentAnalytics {
    pub(super) fn from_challenge(ctx: &ChallengeContext, analytics: &Option<Analytics>) -> Self {
        Self {
            analytics: analytics.clone(),
            network: ctx.network.as_str().to_string(),
            amount: ctx.amount.clone(),
            currency: ctx.currency.clone(),
            intent: if ctx.is_session { "session" } else { "charge" }.to_string(),
        }
    }

    pub(super) fn track_started(&self) {
        if let Some(ref a) = self.analytics {
            a.track(
                analytics::Event::PaymentStarted,
                analytics::PaymentStartedPayload {
                    network: self.network.clone(),
                    amount: self.amount.clone(),
                    currency: self.currency.clone(),
                    intent: self.intent.clone(),
                },
            );
        }
    }

    pub(super) fn track_success(
        &self,
        tx_hash: String,
        session_id: Option<String>,
        url: &str,
        method: &str,
        status_code: u16,
    ) {
        if let Some(ref a) = self.analytics {
            a.track(
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
            a.track(
                analytics::Event::QuerySuccess,
                analytics::QuerySuccessPayload {
                    url: crate::util::redact_url(url),
                    method: method.to_string(),
                    status_code,
                },
            );
        }
    }

    pub(super) fn track_failure(&self, err: &anyhow::Error) {
        if let Some(ref a) = self.analytics {
            a.track(
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
}
