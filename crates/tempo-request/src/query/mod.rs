//! Query command: HTTP request with automatic payment handling.
//!
//! Contains the main `run()` entry point plus request building, output
//! rendering, analytics helpers, and payment challenge parsing.

pub(crate) mod analytics;
pub(crate) mod challenge;
pub(crate) mod headers;
pub(crate) mod output;
pub(crate) mod payload;
pub(crate) mod prepare;
pub(crate) mod sse;

use alloy::primitives::Address;

use crate::{
    args::QueryArgs,
    payment::{router::dispatch_payment, types::PaymentResult},
};
use tempo_common::{
    cli::{context::Context, output::emit_by_format},
    error::{NetworkError, PaymentError, TempoError},
    security::redact_url,
};

use self::output::{build_output_options, write_meta_if_requested};

/// Execute an HTTP request with automatic payment handling.
///
/// This is the main request flow for the `query` command:
/// 1. Send the initial HTTP request
/// 2. If non-402, display the response
/// 3. If 402, detect payment protocol and intent
/// 4. Ensure wallet is available (prompt login if needed)
/// 5. Dispatch to charge or session payment flow
/// 6. Display the final response
pub(crate) async fn run(ctx: &Context, query: QueryArgs) -> Result<(), TempoError> {
    // Offline mode: fail fast before any network I/O
    if query.offline {
        return Err(NetworkError::OfflineMode.into());
    }

    let prepared = prepare::prepare(ctx, &query)?;
    let output_opts = build_output_options(ctx.output_format, ctx.verbosity, &query, &prepared.url);
    let target_url = prepared.url.to_string();
    let method_str = prepared.http.method().to_string();

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

    let challenge = challenge::parse_payment_challenge(&response)?;

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
    if let Some(ref cur) = query.max_pay_currency {
        if !max_pay_currency_matches(cur, &challenge.currency, challenge.network) {
            // Intentional business-rule message: there is no underlying source error
            // object to preserve here, and wording is compatibility-sensitive.
            return Err(PaymentError::PaymentRejected {
                reason: "requested currency does not match client max-pay-currency".to_string(),
                status_code: 402,
            }
            .into());
        }
    }
    if let Some(max_val) = query.max_pay {
        if let Ok(req_val) = challenge.amount.parse::<u128>() {
            if req_val > max_val {
                // Intentional business-rule message for stable CLI UX.
                return Err(PaymentError::PaymentRejected {
                    reason: "price exceeds client max".to_string(),
                    status_code: 402,
                }
                .into());
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
        query.max_pay,
    )
    .await;

    match result {
        Ok(PaymentResult {
            tx_hash,
            session_id,
            status_code,
            response,
        }) => {
            if let Some(wallet_address) = ctx.keys.wallet_address_parsed() {
                ctx.keys
                    .mark_provisioned_address(challenge_network, wallet_address);
            } else {
                tracing::warn!(
                    "skipping provisioned persistence: active wallet address is invalid"
                );
            }
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
            let err = e;
            pay_analytics.track_failure(&err);
            Err(err)
        }
    }
}

fn max_pay_currency_matches(
    requested: &str,
    challenge_currency: &str,
    network: tempo_common::network::NetworkId,
) -> bool {
    let requested = requested.trim();
    if requested.is_empty() {
        return false;
    }

    if let Ok(requested_address) = requested.parse::<Address>() {
        if let Ok(challenge_address) = challenge_currency.trim().parse::<Address>() {
            return requested_address == challenge_address;
        }
        return requested_address == network.token().address;
    }

    requested.eq_ignore_ascii_case(challenge_currency)
        || requested.eq_ignore_ascii_case(network.token().symbol)
}

#[cfg(test)]
mod tests {
    use super::max_pay_currency_matches;
    use tempo_common::network::NetworkId;

    #[test]
    fn max_pay_currency_matches_symbol_case_insensitive() {
        assert!(max_pay_currency_matches("usdc", "USDC", NetworkId::Tempo));
    }

    #[test]
    fn max_pay_currency_matches_network_token_address() {
        let token_addr = format!("{:#x}", NetworkId::Tempo.token().address);
        assert!(max_pay_currency_matches(
            &token_addr,
            "USDC",
            NetworkId::Tempo
        ));
    }

    #[test]
    fn max_pay_currency_matches_challenge_address() {
        let addr = "0x20c000000000000000000000b9537d11c60e8b50";
        assert!(max_pay_currency_matches(addr, addr, NetworkId::Tempo));
    }

    #[test]
    fn max_pay_currency_rejects_mismatch() {
        assert!(!max_pay_currency_matches(
            "pathUSD",
            "USDC",
            NetworkId::Tempo
        ));
    }
}
