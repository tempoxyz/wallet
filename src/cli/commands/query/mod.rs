//! Query command: HTTP request with automatic payment handling.
//!
//! This module implements the `query` command — the primary CLI flow.
//! It sends the initial HTTP request, detects 402 Payment Required responses,
//! dispatches to the charge or session payment path, handles wallet login
//! prompting, and displays the final response.

mod analytics;
mod challenge;
mod data;
mod headers;
mod response;
mod streaming;

use anyhow::Result;
use base64::Engine;

use crate::cli::args::QueryArgs;
use crate::cli::output::OutputOptions;
use crate::cli::Cli;
use crate::cli::Context;
use crate::error::TempoWalletError;
use crate::http::{HttpClient, HttpRequestPlan, DEFAULT_USER_AGENT};
use crate::network::NetworkId;
use crate::payment::dispatch::{dispatch_payment, PaymentResult};
use crate::util::redact_url;

use data::{
    append_data_to_query, join_form_pairs, parse_and_validate_url, parse_data_urlencode,
    resolve_method_and_body,
};
use headers::{has_header, parse_headers, should_auto_add_json_content_type, validate_header_size};
use response::write_meta_if_requested;

/// Default HTTP status codes considered transient/retryable (curl parity).
const DEFAULT_RETRY_STATUS_CODES: &[u16] = &[408, 429, 500, 502, 503, 504];

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

    let mut parsed_url = parse_and_validate_url(&query.url)?;

    // Support -G/--get: append -d and --data-urlencode to query string and force GET if no explicit -X
    if query.get && (!query.data.is_empty() || !query.data_urlencode.is_empty()) {
        append_data_to_query(&mut parsed_url, &query.data, &query.data_urlencode)?;
    }

    let http = build_client(&ctx.cli, &query)?;
    let output_opts = build_output_options(&ctx.cli, &query, &parsed_url);
    let target_url = parsed_url.to_string();
    let method_str = http.plan.method.to_string();

    let sanitized_url = redact_url(&target_url);

    analytics::track_query_started(ctx, &sanitized_url, &method_str);

    if http.log_enabled() {
        eprintln!("Making {method_str} request to: {sanitized_url}");
    }

    // Streaming/SSE mode: perform a streaming request and return.
    if query.is_streaming() {
        return streaming::run_streaming(&http, &target_url, &output_opts, query.sse_json).await;
    }

    // Single execution; retry policy is handled inside HttpClient
    let start = std::time::Instant::now();
    let response = match http.execute(&target_url, /* extra_headers */ &[]).await {
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
        response::render_and_save(&output_opts, response, None)?;
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
    if http.dry_run && query.price_json {
        let obj = serde_json::json!({
            "intent": challenge.intent_str(),
            "network": challenge.network.as_str(),
            "amount": challenge.amount,
            "currency": challenge.currency,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    if http.log_enabled() {
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
                anyhow::bail!(TempoWalletError::PaymentRejected {
                    reason: "price exceeds client max".to_string(),
                    status_code: 402,
                });
            }
        }
    }

    // Skip wallet login for dry-run or when a private key is provided directly
    if !http.dry_run && !ctx.keys.ephemeral {
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
        &http,
        is_session,
        &effective_url,
        challenge.challenge,
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
                    response::display_receipt(
                        &output_opts,
                        &resp,
                        challenge_network,
                        &amount_display,
                    );
                }

                response::render_and_save(&output_opts, resp, query.save_receipt.as_deref())?;
            }
            Ok(())
        }
        Err(e) => {
            pay_analytics.track_failure(&e);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// CLI → domain conversion (request building)
// ---------------------------------------------------------------------------

/// Build a `HttpClient` from CLI arguments.
///
/// This is the boundary where CLI-specific types are converted into
/// domain types used by the HTTP and payment layers.
fn build_client(cli: &Cli, query: &QueryArgs) -> Result<HttpClient> {
    for header in &query.headers {
        validate_header_size(header)?;
        if header.contains('\r') || header.contains('\n') {
            anyhow::bail!(TempoWalletError::InvalidHeader(
                "header contains CR/LF characters".to_string()
            ));
        }
    }

    // Kept as Option so the payment dispatch only enforces network matching
    // when the user explicitly passed --network.
    let network = cli
        .network
        .as_deref()
        .map(|s| s.parse::<NetworkId>().map_err(|e| anyhow::anyhow!(e)))
        .transpose()?;

    let verbosity = cli.verbosity();

    // Determine method/body. HEAD and -G modes suppress the body; otherwise use full inputs.
    let suppress_body = query.head || query.get;
    let method_override = if query.head {
        Some("HEAD")
    } else if query.get && query.method.is_none() {
        Some("GET")
    } else {
        query.method.as_deref()
    };
    let (data, json, toon) = if suppress_body {
        (&[][..], None, None)
    } else {
        (
            query.data.as_slice(),
            query.json.as_deref(),
            query.toon.as_deref(),
        )
    };
    let (method, body) = resolve_method_and_body(method_override, data, json, toon)?;

    let headers = build_extra_headers(query, suppress_body, data);

    // If not using -G, merge --data-urlencode into body (form-encoded)
    let body = if !query.get && !query.data_urlencode.is_empty() {
        let mut base = body.unwrap_or_default();
        let enc_pairs = parse_data_urlencode(&query.data_urlencode)?;
        let form = join_form_pairs(&enc_pairs);
        if !base.is_empty() {
            base.push(b'&');
        }
        base.extend_from_slice(form.as_bytes());
        Some(base)
    } else {
        body
    };

    // Build retry policy from CLI flags
    let mut retry_codes: Vec<u16> = query
        .retry_http
        .as_deref()
        .map(|s| s.split(',').filter_map(|s| s.trim().parse().ok()).collect())
        .unwrap_or_default();
    // Curl parity: when --retries is set but no explicit --retry-http, use default transient set
    if query.retries.is_some() && retry_codes.is_empty() {
        retry_codes = DEFAULT_RETRY_STATUS_CODES.to_vec();
    }

    let plan = HttpRequestPlan {
        method,
        headers,
        body,
        timeout_secs: query.max_time,
        connect_timeout_secs: query.connect_timeout,
        follow_redirects: query.location,
        follow_redirects_limit: query.max_redirs.map(|v| v as usize),
        user_agent: query
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
        insecure: query.insecure,
        proxy: query.proxy.clone(),
        no_proxy: query.no_proxy,
        http2: query.http2,
        http1_only: query.http1_1,
        max_retries: query.retries.unwrap_or(0),
        base_backoff_ms: query.retry_backoff_ms.unwrap_or(250),
        max_backoff_ms: 10_000,
        retry_status_codes: retry_codes,
        // Curl parity: honor Retry-After by default when --retries is used
        honor_retry_after: query.retries.is_some() || query.retry_after,
        // Curl default has exponential backoff without jitter; only apply when user opts in
        retry_jitter_pct: query.retry_jitter,
    };

    HttpClient::new(plan, verbosity, network, query.dry_run)
}

/// Build `OutputOptions` from CLI arguments + config.
///
/// Accepts the already-parsed URL to avoid redundant parsing.
fn build_output_options(cli: &Cli, query: &QueryArgs, parsed_url: &url::Url) -> OutputOptions {
    OutputOptions {
        output_format: cli.resolve_output_format(),
        // -I (HEAD) implies showing headers, even if -i wasn't explicitly set
        include_headers: query.include_headers || query.head,
        output_file: if query.output.is_none() && query.remote_name {
            // Derive a filename from the URL's last path segment; fallback to 'index.html'
            let seg = parsed_url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|v| !v.is_empty())
                .unwrap_or("index.html");
            Some(seg.to_string())
        } else {
            query.output.clone()
        },
        verbosity: cli.verbosity(),
        dump_headers: query.dump_header.clone(),
        write_meta: query.write_meta.clone(),
    }
}

/// Build extra headers (auth, referer, compressed, content-type) on top of
/// the raw user-supplied headers.
fn build_extra_headers(
    query: &QueryArgs,
    suppress_body: bool,
    data: &[String],
) -> Vec<(String, String)> {
    let raw_headers = &query.headers;
    let mut headers = parse_headers(raw_headers);
    // Add Authorization: Basic ... if -u/--user provided and not explicitly overridden by -H
    if let Some(ref user) = query.user {
        if !has_header(raw_headers, "authorization") {
            let encoded = base64::engine::general_purpose::STANDARD.encode(user);
            headers.push(("authorization".to_string(), format!("Basic {}", encoded)));
        }
    }
    // Add Authorization: Bearer if provided and not explicitly overridden
    if let Some(ref token) = query.bearer {
        if !has_header(raw_headers, "authorization") && query.user.is_none() {
            headers.push(("authorization".to_string(), format!("Bearer {}", token)));
        }
    }
    // Add Referer header if provided and not overridden via -H
    if let Some(ref referer) = query.referer {
        if !has_header(raw_headers, "referer") {
            headers.push(("referer".to_string(), referer.clone()));
        }
    }
    // Add Accept-Encoding on --compressed (reqwest negotiates automatically; header makes intent explicit)
    if query.compressed && !has_header(raw_headers, "accept-encoding") {
        headers.push(("accept-encoding".to_string(), "gzip, br".to_string()));
    }
    if !suppress_body {
        if should_auto_add_json_content_type(
            raw_headers,
            query.json.as_deref(),
            query.toon.as_deref(),
            data,
        ) {
            headers.push(("content-type".to_string(), "application/json".to_string()));
        } else if !query.data_urlencode.is_empty() && !has_header(raw_headers, "content-type") {
            headers.push((
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ));
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use url::Url;

    use crate::cli::args::Commands;
    use crate::cli::output::OutputFormat;

    /// Parse a CLI invocation into both `Cli` and `QueryArgs` for testing.
    fn parse(args: &[&str]) -> (Cli, QueryArgs) {
        let all: Vec<&str> = std::iter::once("tempo-wallet")
            .chain(std::iter::once("query"))
            .chain(args.iter().copied())
            .collect();
        let mut cli = Cli::try_parse_from(all).unwrap();
        let query = match cli.command.take() {
            Some(Commands::Query(q)) => *q,
            _ => panic!("expected Query command"),
        };
        (cli, query)
    }

    #[test]
    fn remote_name_derives_filename_from_url() {
        let (c, q) = parse(&["-O", "https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_file.as_deref(), Some("file.txt"));
    }

    #[test]
    fn remote_name_falls_back_to_index_html() {
        let (c, q) = parse(&["-O", "https://example.com/"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_file.as_deref(), Some("index.html"));
    }

    #[test]
    fn head_implies_include_headers() {
        let (c, q) = parse(&["-I", "https://example.com"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert!(opts.include_headers);
    }

    #[test]
    fn explicit_output_file_overrides_remote_name() {
        let (c, q) = parse(&["-o", "custom.txt", "https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_file.as_deref(), Some("custom.txt"));
    }

    #[test]
    fn no_output_flags_means_no_file() {
        let (c, q) = parse(&["https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert!(opts.output_file.is_none());
        assert!(!opts.include_headers);
        assert_eq!(opts.output_format, OutputFormat::Text);
    }

    #[test]
    fn json_output_flag() {
        let (c, q) = parse(&["-j", "https://example.com"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_format, OutputFormat::Json);
    }
}
