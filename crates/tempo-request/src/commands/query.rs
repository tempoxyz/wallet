//! Query command: HTTP request with automatic payment handling.
//!
//! Sends the initial HTTP request, detects 402 Payment Required responses,
//! dispatches to the charge or session payment path, handles wallet login
//! prompting, and displays the final response.

use anyhow::Result;

use crate::args::QueryArgs;
use crate::payment::router::{dispatch_payment, PaymentResult};
use tempo_common::cli::context::Context;
use tempo_common::cli::output::emit_by_format;
use tempo_common::error::{NetworkError, PaymentError};
use tempo_common::security::redact_url;

use super::analytics;
use super::output::{self, build_output_options, write_meta_if_requested};
use super::payment_challenge;
use super::prepare;
use super::sse;

/// Execute an HTTP request with automatic payment handling.
///
/// This is the main request flow for the `query` command:
/// 1. Send the initial HTTP request
/// 2. If non-402, display the response
/// 3. If 402, detect payment protocol and intent
/// 4. Ensure wallet is available (prompt login if needed)
/// 5. Dispatch to charge or session payment flow
/// 6. Display the final response
pub(crate) async fn run(ctx: &Context, query: QueryArgs) -> Result<()> {
    // Offline mode: fail fast before any network I/O
    if query.offline {
        anyhow::bail!(NetworkError::OfflineMode);
    }

    let prepared = prepare::prepare(ctx, &query)?;
    let output_opts = build_output_options(ctx.output_format, ctx.verbosity, &query, &prepared.url);
    let target_url = prepared.url.to_string();
    let method_str = prepared.http.plan.method.to_string();

    let sanitized_url = redact_url(&target_url);

    analytics::track_query_started(ctx, &sanitized_url, &method_str);

    if prepared.http.log_enabled() {
        eprintln!("Making {method_str} request to: {sanitized_url}");
    }

    // Streaming/SSE mode: perform a streaming request and return.
    if query.is_streaming() {
        return sse::run(&prepared.http, &target_url, &output_opts, query.sse_json).await;
    }

    // Single execution; retry policy is handled inside HttpClient
    let start = std::time::Instant::now();
    let response = match prepared
        .http
        .execute(&target_url, /* extra_headers */ &[])
        .await
    {
        Ok(r) => r,
        Err(e) => {
            analytics::track_query_failure(ctx, &sanitized_url, &method_str, &e.to_string());
            return Err(e);
        }
    };
    // Write meta for immediate response (non-402) if requested
    if let Err(e) = write_meta_if_requested(
        &output_opts,
        response.status_code,
        &response.headers,
        start.elapsed().as_millis(),
        response.body.len(),
        response.final_url.as_deref().unwrap_or(&target_url),
    ) {
        tracing::warn!("failed to write response metadata: {e}");
    }

    if response.status_code != 402 {
        analytics::track_query_success(ctx, &sanitized_url, &method_str, response.status_code);
        output::handle_response(&output_opts, response, None)?;
        return Ok(());
    }

    // Use the final URL after redirects for payment retry, not the original URL.
    // This prevents a malicious redirector from capturing payment credentials:
    // attacker.example → 307 → paid.example (402) → retry must go to paid.example.
    let effective_url = response
        .final_url
        .as_deref()
        .unwrap_or(&target_url)
        .to_string();

    let challenge = payment_challenge::parse_payment_challenge(&response)?;

    // Dry-run price output for agents
    if prepared.http.dry_run && query.price_json {
        let obj = serde_json::json!({
            "intent": challenge.intent_str(),
            "network": challenge.network.as_str(),
            "amount": challenge.amount,
            "currency": challenge.currency,
        });
        emit_by_format(output_opts.output_format, &obj, || {
            println!("{}", serde_json::to_string_pretty(&obj)?);
            Ok(())
        })?;
        return Ok(());
    }

    if prepared.http.log_enabled() {
        eprintln!(
            "Payment required: intent={} network={} amount={}",
            challenge.intent_str(),
            challenge.network.as_str(),
            challenge.amount_display(),
        );
    }

    // Enforce client-side price cap if configured
    if let Some(max_val) = query.max_pay {
        if let Ok(req_val) = challenge.amount.parse::<u128>() {
            // Optional currency match if provided
            let currency_ok = query.max_pay_currency.as_ref().is_none_or(|cur| {
                let symbol = challenge.network.token().symbol;
                let cur_lower = cur.to_lowercase();
                cur_lower == challenge.currency.to_lowercase() || cur_lower == symbol.to_lowercase()
            });
            if currency_ok && req_val > max_val {
                anyhow::bail!(PaymentError::PaymentRejected {
                    reason: "price exceeds client max".to_string(),
                    status_code: 402,
                });
            }
        }
    }

    // Skip wallet login for dry-run or when a private key is provided directly
    if !prepared.http.dry_run && !ctx.keys.ephemeral {
        ctx.keys.ensure_key_for_network(challenge.network)?;
    }

    // Capture display values before `challenge` is moved into dispatch_payment.
    let is_session = challenge.is_session;
    let challenge_network = challenge.network;
    let amount_display = challenge.amount_display();

    let pay_analytics = analytics::PaymentAnalytics::new(
        ctx,
        challenge_network.as_str(),
        &challenge.amount,
        &challenge.currency,
        challenge.intent_str(),
    );
    pay_analytics.track_started();

    let result = dispatch_payment(
        &ctx.config,
        &prepared.http,
        is_session,
        &effective_url,
        challenge.challenge,
        challenge_network,
        &ctx.keys,
    )
    .await;

    match result {
        Ok(PaymentResult {
            tx_hash,
            session_id,
            status_code,
            response,
        }) => {
            ctx.keys
                .mark_provisioned(challenge_network, ctx.keys.wallet_address());
            pay_analytics.track_success(tx_hash, session_id, &target_url, &method_str, status_code);
            if let Some(resp) = response {
                // Display receipt summary for charge responses
                if !is_session {
                    output::display_receipt(
                        &output_opts,
                        &resp,
                        challenge_network,
                        &amount_display,
                    );
                }

                output::handle_response(&output_opts, resp, query.save_receipt.as_deref())?;
            }
            Ok(())
        }
        Err(e) => {
            pay_analytics.track_failure(&e);
            Err(e)
        }
    }
}
