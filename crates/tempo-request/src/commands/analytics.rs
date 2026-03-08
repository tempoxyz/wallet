//! Payment analytics tracking for query command flows.

use crate::analytics::{
    PaymentFailurePayload, PaymentStartedPayload, PaymentSuccessPayload, QueryFailurePayload,
    QueryStartedPayload, QuerySuccessPayload,
};
use tempo_common::analytics::Event;
use tempo_common::cli::context::Context;
use tempo_common::redact::sanitize_error;

// ---------------------------------------------------------------------------
// Pre-402 query tracking (no payment context needed)
// ---------------------------------------------------------------------------

pub(super) fn track_query_started(ctx: &Context, url: &str, method: &str) {
    ctx.track(
        Event::QueryStarted,
        QueryStartedPayload {
            url: url.to_string(),
            method: method.to_string(),
        },
    );
}

pub(super) fn track_query_failure(ctx: &Context, url: &str, method: &str, error: &str) {
    ctx.track(
        Event::QueryFailure,
        QueryFailurePayload {
            url: url.to_string(),
            method: method.to_string(),
            error: sanitize_error(error),
        },
    );
}

pub(super) fn track_query_success(ctx: &Context, url: &str, method: &str, status_code: u16) {
    ctx.track(
        Event::QuerySuccess,
        QuerySuccessPayload {
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
    network: &'a str,
    amount: &'a str,
    currency: &'a str,
    intent: &'static str,
}

impl<'a> PaymentAnalytics<'a> {
    pub(super) fn new(
        ctx: &'a Context,
        network: &'a str,
        amount: &'a str,
        currency: &'a str,
        intent: &'static str,
    ) -> Self {
        Self {
            ctx,
            network,
            amount,
            currency,
            intent,
        }
    }

    pub(super) fn track_started(&self) {
        self.ctx.track(
            Event::PaymentStarted,
            PaymentStartedPayload {
                network: self.network.to_string(),
                amount: self.amount.to_string(),
                currency: self.currency.to_string(),
                intent: self.intent.to_string(),
            },
        );
    }

    /// Track a successful payment.
    ///
    /// Also fires a `QuerySuccess` event so the overall request is counted as
    /// successful (the non-402 path fires this directly from `mod.rs`).
    pub(super) fn track_success(
        &self,
        tx_hash: String,
        session_id: Option<String>,
        url: &str,
        method: &str,
        status_code: u16,
    ) {
        self.ctx.track(
            Event::PaymentSuccess,
            PaymentSuccessPayload {
                network: self.network.to_string(),
                amount: self.amount.to_string(),
                currency: self.currency.to_string(),
                intent: self.intent.to_string(),
                tx_hash,
                session_id,
            },
        );
        track_query_success(self.ctx, url, method, status_code);
    }

    pub(super) fn track_failure(&self, err: &anyhow::Error) {
        self.ctx.track(
            Event::PaymentFailure,
            PaymentFailurePayload {
                network: self.network.to_string(),
                amount: self.amount.to_string(),
                currency: self.currency.to_string(),
                intent: self.intent.to_string(),
                error: sanitize_error(&err.to_string()),
            },
        );
    }
}
