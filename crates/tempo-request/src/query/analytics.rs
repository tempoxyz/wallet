//! Payment analytics tracking for query command flows.

use std::fmt::Display;

use crate::analytics::{
    PaymentFailurePayload, PaymentStartedPayload, PaymentSuccessPayload, QueryFailurePayload,
    QueryStartedPayload, QuerySuccessPayload,
};
use tempo_common::{analytics::Event, cli::context::Context, security::sanitize_error};

const QUERY_STARTED: Event = Event::new("query started");
const QUERY_SUCCESS: Event = Event::new("query succeeded");
const QUERY_FAILURE: Event = Event::new("query failed");
const PAYMENT_STARTED: Event = Event::new("payment started");
const PAYMENT_SUCCESS: Event = Event::new("payment succeeded");
const PAYMENT_FAILURE: Event = Event::new("payment failed");

// ---------------------------------------------------------------------------
// Pre-402 query tracking (no payment context needed)
// ---------------------------------------------------------------------------

pub(crate) fn track_query_started(ctx: &Context, url: &str, method: &str) {
    ctx.track(
        QUERY_STARTED,
        QueryStartedPayload {
            url: url.to_string(),
            method: method.to_string(),
        },
    );
}

pub(crate) fn track_query_failure(ctx: &Context, url: &str, method: &str, error: &str) {
    ctx.track(
        QUERY_FAILURE,
        QueryFailurePayload {
            url: url.to_string(),
            method: method.to_string(),
            error: sanitize_error(error),
        },
    );
}

pub(crate) fn track_query_success(ctx: &Context, url: &str, method: &str, status_code: u16) {
    ctx.track(
        QUERY_SUCCESS,
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
pub(crate) struct PaymentAnalytics<'a> {
    ctx: &'a Context,
    url: &'a str,
    network: &'a str,
    amount: &'a str,
    currency: &'a str,
    intent: &'static str,
}

impl<'a> PaymentAnalytics<'a> {
    pub(crate) const fn new(
        ctx: &'a Context,
        url: &'a str,
        network: &'a str,
        amount: &'a str,
        currency: &'a str,
        intent: &'static str,
    ) -> Self {
        Self {
            ctx,
            url,
            network,
            amount,
            currency,
            intent,
        }
    }

    pub(crate) fn track_started(&self) {
        self.ctx.track(
            PAYMENT_STARTED,
            PaymentStartedPayload {
                url: self.url.to_string(),
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
    pub(crate) fn track_success(
        &self,
        tx_hash: Option<String>,
        session_id: Option<String>,
        url: &str,
        method: &str,
        status_code: u16,
    ) {
        self.ctx.track(
            PAYMENT_SUCCESS,
            PaymentSuccessPayload {
                url: self.url.to_string(),
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

    pub(crate) fn track_failure(&self, err: &impl Display) {
        self.ctx.track(
            PAYMENT_FAILURE,
            PaymentFailurePayload {
                url: self.url.to_string(),
                network: self.network.to_string(),
                amount: self.amount.to_string(),
                currency: self.currency.to_string(),
                intent: self.intent.to_string(),
                error: sanitize_error(&err.to_string()),
            },
        );
    }
}
