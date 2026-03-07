//! Query command: HTTP request with automatic payment handling.
//!
//! This module implements the `query` command — the primary CLI flow.
//! It sends the initial HTTP request, detects 402 Payment Required responses,
//! dispatches to the charge or session payment path, handles wallet login
//! prompting, and displays the final response.

mod analytics;
mod challenge;
mod context;
mod input;
mod receipt;
mod streaming;

use anyhow::{Context as _, Result};

use crate::analytics::{Event, QueryFailurePayload, QueryStartedPayload, QuerySuccessPayload};
use crate::cli::args::QueryArgs;
use crate::cli::Context;
use crate::error::TempoWalletError;
use crate::payment::dispatch::{dispatch_payment, PaymentResult};
use crate::util::{format_token_amount, redact_url, sanitize_error};
use input::resolve_data;
use receipt::write_meta_if_requested;

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
        anyhow::bail!(TempoWalletError::OfflineMode);
    }

    // Validate the URL early to give a clear error instead of a cryptic reqwest message.
    let mut parsed_url = match url::Url::parse(&query.url) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            if scheme != "http" && scheme != "https" {
                anyhow::bail!(TempoWalletError::InvalidUrl(format!(
                    "unsupported scheme '{}'",
                    scheme
                )));
            }
            parsed
        }
        Err(e) => {
            anyhow::bail!(TempoWalletError::InvalidUrl(e.to_string()));
        }
    };

    // Support -G/--get: append -d and --data-urlencode to query string and force GET if no explicit -X
    if query.get && (!query.data.is_empty() || !query.data_urlencode.is_empty()) {
        // Raw -d data (verbatim, joined by '&')
        let mut raw = String::new();
        if !query.data.is_empty() {
            let mut combined: Vec<u8> = Vec::new();
            for item in &query.data {
                let bytes = resolve_data(item)?;
                if !combined.is_empty() {
                    combined.push(b'&');
                }
                combined.extend(bytes);
            }
            raw = String::from_utf8(combined).context("data is not valid UTF-8 for --get")?;
        }
        // Encoded data from --data-urlencode
        let enc_pairs = input::parse_data_urlencode(&query.data_urlencode)?;
        let mut enc_joined: Vec<String> = Vec::new();
        for (name, val) in enc_pairs {
            if let Some(n) = name {
                enc_joined.push(format!("{}={}", n, val));
            } else {
                enc_joined.push(val);
            }
        }
        let appended = if raw.is_empty() {
            enc_joined.join("&")
        } else if enc_joined.is_empty() {
            raw
        } else {
            format!("{}&{}", raw, enc_joined.join("&"))
        };
        let new_query = match parsed_url.query() {
            Some(q) if !q.is_empty() => format!("{q}&{appended}"),
            _ => appended,
        };
        parsed_url.set_query(Some(&new_query));
    }

    let http = context::build_http_client(&ctx.cli, &query)?;
    let output_opts = context::build_output_options(&ctx.cli, &query, &parsed_url);
    let target_url: String = parsed_url.into();
    let method_str = http.plan.method.to_string();

    let sanitized_url = redact_url(&target_url);

    if let Some(ref a) = ctx.analytics {
        a.track(
            Event::QueryStarted,
            QueryStartedPayload {
                url: sanitized_url.clone(),
                method: method_str.clone(),
            },
        );
    }

    if http.log_enabled() {
        eprintln!("Making {} request to: {}", http.plan.method, sanitized_url);
    }

    // Streaming/SSE mode: perform a streaming request and return.
    if query.stream || query.sse || query.sse_json {
        return streaming::execute_streaming(&http, &target_url, &output_opts, query.sse_json)
            .await;
    }

    // Single execution; retry policy is handled inside HttpClient
    let start = std::time::Instant::now();
    let response = match http.execute(&target_url, &[]).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(ref a) = ctx.analytics {
                a.track(
                    Event::QueryFailure,
                    QueryFailurePayload {
                        url: sanitized_url.clone(),
                        method: method_str.clone(),
                        error: sanitize_error(&e.to_string()),
                    },
                );
            }
            return Err(e);
        }
    };
    // Write meta for immediate response (non-402) if requested
    if let Err(e) = write_meta_if_requested(
        &output_opts,
        &response,
        start.elapsed().as_millis(),
        response.body.len(),
        response.final_url.as_deref().unwrap_or(&target_url),
    ) {
        tracing::warn!("failed to write response metadata: {e}");
    }

    if response.status_code != 402 {
        if let Some(ref a) = ctx.analytics {
            a.track(
                Event::QuerySuccess,
                QuerySuccessPayload {
                    url: sanitized_url,
                    method: method_str,
                    status_code: response.status_code,
                },
            );
        }
        receipt::finalize_response(&output_opts, response)?;
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

    let challenge_ctx = challenge::parse_payment_challenge(&response)?;

    // Dry-run price output for agents
    if http.dry_run && query.price_json {
        let obj = serde_json::json!({
            "intent": if challenge_ctx.is_session { "session" } else { "charge" },
            "network": challenge_ctx.network.as_str(),
            "amount": challenge_ctx.amount,
            "currency": challenge_ctx.currency,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    if http.log_enabled() {
        let intent = if challenge_ctx.is_session {
            "session"
        } else {
            "charge"
        };
        let amount_display = challenge_ctx
            .amount
            .parse::<u128>()
            .ok()
            .map(|a| format_token_amount(a, challenge_ctx.network))
            .unwrap_or_else(|| challenge_ctx.amount.clone());
        eprintln!(
            "Payment required: intent={intent} network={} amount={amount_display}",
            challenge_ctx.network.as_str()
        );
    }

    // Enforce client-side price cap if configured
    if let Some(max) = &query.max_pay {
        let max_val: u128 = max.parse().map_err(|_| {
            TempoWalletError::InvalidConfig(format!("invalid --max-pay value: {max}"))
        })?;
        if let Ok(req_val) = challenge_ctx.amount.parse::<u128>() {
            // Optional currency match if provided
            let currency_ok = query.max_pay_currency.as_ref().is_none_or(|cur| {
                let symbol = challenge_ctx.network.token().symbol;
                let cur_lower = cur.to_lowercase();
                cur_lower == challenge_ctx.currency.to_lowercase()
                    || cur_lower == symbol.to_lowercase()
            });
            if currency_ok && req_val > max_val {
                anyhow::bail!(TempoWalletError::PaymentRejected {
                    reason: "price exceeds client max".to_string(),
                    status_code: 402,
                });
            }
        }
    }

    // Skip wallet login for dry-run or when a private key is provided directly
    if !http.dry_run && !ctx.keys.ephemeral {
        challenge::ensure_wallet_configured(&ctx.keys, challenge_ctx.network)?;
    }

    let pay_analytics = analytics::PaymentAnalytics::from_challenge(&challenge_ctx, &ctx.analytics);
    pay_analytics.track_started();

    let result = dispatch_payment(
        &ctx.config,
        &http,
        challenge_ctx.is_session,
        &effective_url,
        challenge_ctx.challenge,
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
                .mark_provisioned(challenge_ctx.network, ctx.keys.wallet_address());
            pay_analytics.track_success(tx_hash, session_id, &target_url, &method_str, status_code);
            if let Some(resp) = response {
                // Capture receipt header before consuming response for output
                let receipt_hdr = resp.header("payment-receipt").map(|s| s.to_string());
                // Display receipt summary for charge responses
                if !challenge_ctx.is_session {
                    receipt::display_receipt(
                        &output_opts,
                        &resp,
                        challenge_ctx.network,
                        &challenge_ctx.amount,
                    );
                }

                receipt::finalize_response(&output_opts, resp)?;
                // Optionally save receipt JSON if present
                if let (Some(path), Some(h)) = (query.save_receipt.as_ref(), receipt_hdr.as_ref()) {
                    match mpp::parse_receipt(h) {
                        Ok(receipt) => {
                            let s = serde_json::to_string_pretty(&receipt)?;
                            std::fs::write(path, s)?;
                        }
                        Err(e) => {
                            tracing::warn!("failed to parse receipt for --save-receipt: {e}");
                        }
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            pay_analytics.track_failure(&e);
            Err(e)
        }
    }
}
