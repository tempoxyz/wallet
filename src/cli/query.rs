//! Query command: HTTP request with automatic payment handling.
//!
//! This module implements the `query` command — the primary CLI flow.
//! It sends the initial HTTP request, detects 402 Payment Required responses,
//! dispatches to the charge or session payment path, handles wallet login
//! prompting, and displays the final response.

use std::io::IsTerminal;

use anyhow::{Context, Result};
use base64::Engine;
use mpp::protocol::methods::tempo::session::TempoSessionExt;
use mpp::protocol::methods::tempo::TempoChargeExt;
use mpp::PaymentProtocol;

use crate::analytics::{self, Analytics};
use crate::config::{load_config_with_overrides, Config};
use crate::error::PrestoError;
use crate::http::{
    get_request_method_and_body, parse_headers, resolve_data, should_auto_add_json_content_type,
    validate_header_size, HttpRequestPlan, HttpResponse, RequestContext, RequestRuntime,
};
use crate::network::{resolve_token_meta, ExplorerConfig, Network};
use crate::payment::charge::prepare_charge;
use crate::payment::session::{handle_session_request, SessionResult};
use crate::util::{format_token_amount, hyperlink};
use crate::wallet::credentials::{has_credentials_override, WalletCredentials};

use super::output::{handle_regular_response, write_meta_if_requested, OutputOptions};
use super::{Cli, QueryArgs};

/// Parse --data-urlencode items into (name, value) tuples with URL-encoding applied.
fn parse_data_urlencode(items: &[String]) -> Vec<(Option<String>, String)> {
    let mut pairs = Vec::new();
    for it in items {
        if let Some(rest) = it.strip_prefix('@') {
            // @filename — read file contents
            if let Ok(content) = std::fs::read(rest) {
                let enc = urlencoding::encode_binary(&content).to_string();
                pairs.push((None, enc));
            }
            continue;
        }
        if let Some(pos) = it.find("=@") {
            // name@filename pattern (curl-style)
            let (name, file) = it.split_at(pos);
            let file = &file[2..];
            if let Ok(content) = std::fs::read(file) {
                let enc = urlencoding::encode_binary(&content).to_string();
                pairs.push((Some(name.to_string()), enc));
            }
            continue;
        }
        if let Some((name, val)) = it.split_once('=') {
            pairs.push((Some(name.to_string()), urlencoding::encode(val).to_string()));
        } else {
            // raw value; encode as a nameless component
            pairs.push((None, urlencoding::encode(it).to_string()));
        }
    }
    pairs
}
/// Execute an HTTP request with automatic payment handling.
///
/// This is the main request flow for the `query` command:
/// 1. Send the initial HTTP request
/// 2. If non-402, display the response
/// 3. If 402, detect payment protocol and intent
/// 4. Ensure wallet is available (prompt login if needed)
/// 5. Dispatch to charge or session payment flow
/// 6. Display the final response
pub async fn make_request(cli: Cli, query: QueryArgs, analytics: Option<Analytics>) -> Result<()> {
    let mut config = load_config_with_overrides(cli.config.as_ref())?;

    // Apply --rpc flag override to config.
    // The PRESTO_RPC_URL env var is already handled by load_config_with_overrides,
    // but the explicit --rpc flag takes final precedence.
    if let Some(ref rpc_url) = query.rpc_url {
        config.set_rpc_override(rpc_url.clone());
    }

    let mut url = query.url.clone();

    // Validate the URL early to give a clear error instead of a cryptic reqwest message.
    match url::Url::parse(&url) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            if scheme != "http" && scheme != "https" {
                anyhow::bail!(PrestoError::InvalidUrl(format!(
                    "unsupported scheme '{}'",
                    scheme
                )));
            }
        }
        Err(e) => {
            anyhow::bail!(PrestoError::InvalidUrl(e.to_string()));
        }
    }

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
        let enc_pairs = parse_data_urlencode(&query.data_urlencode);
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
        let mut parsed =
            url::Url::parse(&url).context("invalid URL when applying --get query parameters")?;
        let new_query = match parsed.query() {
            Some(q) if !q.is_empty() => format!("{q}&{appended}"),
            _ => appended,
        };
        parsed.set_query(Some(&new_query));
        url = parsed.to_string();
    }

    let request_ctx = build_request_context(&cli, &query)?;
    let output_opts = build_output_options(&cli, &query, &config);
    let method_str = request_ctx.plan.method.to_string();

    let sanitized_url = analytics::sanitize_url(&url);

    if let Some(ref a) = analytics {
        a.track(
            analytics::Event::QueryStarted,
            analytics::QueryStartedPayload {
                url: sanitized_url.clone(),
                method: method_str.clone(),
            },
        );
    }

    if request_ctx.log_enabled() {
        eprintln!("Making {} request to: {url}", request_ctx.plan.method);
    }

    // Streaming/SSE mode: perform a streaming request and return.
    if query.stream || query.sse || query.sse_json {
        return execute_streaming(&request_ctx, &url, &output_opts, query.sse_json).await;
    }

    // Execute with optional HTTP status retries
    let mut attempts_left = query.retries.unwrap_or(0);
    let retry_codes: Vec<u16> = query
        .retry_http
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.trim().parse::<u16>().ok())
        .collect();
    let mut backoff_ms = query.retry_backoff_ms.unwrap_or(250);
    let response = loop {
        let start = std::time::Instant::now();
        let resp_res = request_ctx.execute(&url, None).await;
        match resp_res {
            Ok(r) => {
                // Honor Retry-After and backoff for configured HTTP statuses
                if !retry_codes.is_empty()
                    && retry_codes.contains(&r.status_code)
                    && attempts_left > 0
                {
                    let delay_ms = if query.retry_after {
                        if let Some(ra) = r.get_header("retry-after") {
                            if let Ok(secs) = ra.trim().parse::<u64>() {
                                secs.saturating_mul(1000)
                            } else {
                                backoff_ms
                            }
                        } else {
                            backoff_ms
                        }
                    } else {
                        backoff_ms
                    };
                    // Apply jitter if requested
                    let jittered = if let Some(pct) = query.retry_jitter {
                        let jitter = ((delay_ms as f64) * (pct as f64 / 100.0)) as u64;
                        let rand = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .subsec_nanos()
                            % (jitter as u32)) as u64;
                        delay_ms.saturating_add(rand)
                    } else {
                        delay_ms
                    };

                    tokio::time::sleep(std::time::Duration::from_millis(jittered)).await;
                    attempts_left -= 1;
                    // Exponential backoff with cap
                    backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                    continue;
                }

                // Write meta for immediate response (non-402) if requested
                let _ = write_meta_if_requested(
                    &output_opts,
                    &r,
                    start.elapsed().as_millis(),
                    r.body.len(),
                    r.final_url.as_deref().unwrap_or(&url),
                );
                break r;
            }
            Err(e) => {
                if let Some(ref a) = analytics {
                    a.track(
                        analytics::Event::QueryFailure,
                        analytics::QueryFailurePayload {
                            url: sanitized_url.clone(),
                            method: method_str.clone(),
                            error: analytics::sanitize_error(&e.to_string()),
                        },
                    );
                }
                if attempts_left > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    attempts_left -= 1;
                    backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                    continue;
                }
                return Err(e);
            }
        }
    };

    if !response.is_payment_required() {
        if let Some(ref a) = analytics {
            a.track(
                analytics::Event::QuerySuccess,
                analytics::QuerySuccessPayload {
                    url: sanitized_url,
                    method: method_str,
                    status_code: response.status_code,
                },
            );
        }
        finalize_response(&output_opts, response)?;
        return Ok(());
    }

    // Use the final URL after redirects for payment retry, not the original URL.
    // This prevents a malicious redirector from capturing payment credentials:
    // attacker.example → 307 → paid.example (402) → retry must go to paid.example.
    let effective_url = response.final_url.as_deref().unwrap_or(&url).to_string();

    let challenge_ctx = parse_payment_challenge(&response)?;

    // Dry-run price output for agents
    if request_ctx.runtime.dry_run && query.price_json {
        let obj = serde_json::json!({
            "intent": if challenge_ctx.is_session { "session" } else { "charge" },
            "network": challenge_ctx.network,
            "amount": challenge_ctx.amount,
            "currency": challenge_ctx.currency,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    if request_ctx.log_enabled() {
        let intent = if challenge_ctx.is_session {
            "session"
        } else {
            "charge"
        };
        let (symbol, decimals) =
            resolve_token_meta(&challenge_ctx.network, &challenge_ctx.currency);
        let amount_display = challenge_ctx
            .amount
            .parse::<u128>()
            .ok()
            .map(|a| format_token_amount(a, symbol, decimals))
            .unwrap_or_else(|| challenge_ctx.amount.clone());
        eprintln!(
            "Payment required: intent={intent} network={} amount={amount_display}",
            challenge_ctx.network
        );
    }

    // Enforce client-side price cap if configured
    if let Some(max) = &query.max_pay {
        if let Ok(max_val) = max.parse::<u128>() {
            if let Ok(req_val) = challenge_ctx.amount.parse::<u128>() {
                // Optional currency match if provided
                let mut currency_ok = true;
                if let Some(ref cur) = query.max_pay_currency {
                    let (symbol, _dec) =
                        resolve_token_meta(&challenge_ctx.network, &challenge_ctx.currency);
                    let cur_lower = cur.to_lowercase();
                    currency_ok = cur_lower == challenge_ctx.currency.to_lowercase()
                        || cur_lower == symbol.to_lowercase();
                }
                if currency_ok && req_val > max_val {
                    anyhow::bail!(PrestoError::PaymentRejected {
                        reason: "price exceeds client max".to_string(),
                        status_code: 402,
                    });
                }
            }
        }
    }

    // Skip wallet login for dry-run or when a private key is provided directly
    if !request_ctx.runtime.dry_run && !has_credentials_override() {
        ensure_wallet_or_prompt_login(
            &request_ctx,
            &cli,
            &mut config,
            &analytics,
            &challenge_ctx.network,
        )
        .await?;
    }

    let pay_analytics = PaymentAnalytics::from_challenge(&challenge_ctx, &analytics);
    pay_analytics.track_started();

    let result = dispatch_payment(
        &config,
        &request_ctx,
        &output_opts,
        &challenge_ctx,
        &effective_url,
        &response,
    )
    .await;

    // Auto-login retry: if the error is fixable by login and we're interactive, do it automatically
    let result = match result {
        Err(e)
            if is_login_fixable(&e)
                && std::io::stdin().is_terminal()
                && !has_credentials_override() =>
        {
            eprintln!("Setting up wallet for this network...\n");
            let network = request_ctx
                .runtime
                .network
                .as_deref()
                .or(Some(challenge_ctx.network.as_str()));
            super::auth::run_login(network, analytics.clone(), super::OutputFormat::Text).await?;
            eprintln!("\nRetrying payment...");

            let config = load_config_with_overrides(cli.config.as_ref())?;
            dispatch_payment(
                &config,
                &request_ctx,
                &output_opts,
                &challenge_ctx,
                &effective_url,
                &response,
            )
            .await
        }
        other => other,
    };

    match result {
        Ok(result) => {
            WalletCredentials::mark_provisioned(&challenge_ctx.network);
            pay_analytics.track_success(
                result.tx_hash,
                result.session_id.clone(),
                &url,
                &method_str,
                result.status_code,
            );
            if let Some(resp) = result.response {
                // Capture receipt header before consuming response for output
                let receipt_hdr = resp.get_header("payment-receipt").map(|s| s.to_string());
                finalize_response(&output_opts, resp)?;
                // Optionally save receipt JSON if present
                if let (Some(path), Some(h)) = (query.save_receipt.as_ref(), receipt_hdr.as_ref()) {
                    if let Ok(receipt) = mpp::parse_receipt(h) {
                        let s = serde_json::to_string_pretty(&receipt)?;
                        std::fs::write(path, s)?;
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

/// Execute a streaming request and write the body to stdout incrementally.
async fn execute_streaming(
    request_ctx: &RequestContext,
    url: &str,
    output_opts: &OutputOptions,
    sse_json: bool,
) -> Result<()> {
    use futures::StreamExt;
    use std::io::Write;

    let start = std::time::Instant::now();
    let client = request_ctx.build_reqwest_client(None)?;
    let mut req = client.request(request_ctx.plan.method.clone(), url);
    if let Some(ref body) = request_ctx.plan.body {
        req = req.body(body.clone());
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let final_url_string = resp.url().to_string();
    let header_map_for_meta: std::collections::HashMap<String, String> = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.as_str().to_lowercase(), s.to_string()))
        })
        .collect();

    if output_opts.include_headers {
        println!("HTTP {}", status);
        for (k, v) in resp.headers() {
            if let Ok(s) = v.to_str() {
                println!("{}: {}", k.as_str().to_lowercase(), s);
            }
        }
        println!();
    }

    // Handle fail modes
    if status >= 400 && output_opts.fail_silently {
        anyhow::bail!(PrestoError::Http(format!(
            "{} {}",
            status,
            http_status_text(status)
        )));
    }

    let mut bytes_written: usize = 0;
    if sse_json {
        // Convert SSE to NDJSON objects with a simple parser
        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            buf.extend_from_slice(&chunk);
            // Process complete lines
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line = buf.drain(..=pos).collect::<Vec<u8>>();
                let s = String::from_utf8_lossy(&line);
                let st = s.trim_end_matches(['\r', '\n']);
                if st.is_empty() {
                    continue;
                }
                if let Some(rest) = st.strip_prefix("data:") {
                    let obj = serde_json::json!({"data": rest.trim_start()});
                    let out = serde_json::to_string(&obj)?;
                    std::io::stdout().write_all(out.as_bytes())?;
                    std::io::stdout().write_all(b"\n")?;
                    bytes_written = bytes_written.saturating_add(out.len() + 1);
                }
            }
            std::io::stdout().flush().ok();
        }
    } else {
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            bytes_written = bytes_written.saturating_add(chunk.len());
            std::io::stdout().write_all(&chunk)?;
            std::io::stdout().flush().ok();
        }
    }

    // Write meta if requested
    let meta_resp = HttpResponse {
        status_code: status,
        headers: header_map_for_meta,
        body: Vec::new(),
        final_url: Some(final_url_string.clone()),
    };
    let _ = write_meta_if_requested(
        output_opts,
        &meta_resp,
        start.elapsed().as_millis(),
        bytes_written,
        &final_url_string,
    );

    if status >= 400 {
        anyhow::bail!(PrestoError::Http(format!(
            "{} {}",
            status,
            http_status_text(status)
        )));
    }

    Ok(())
}

/// Result of a successful payment dispatch.
struct PaymentResult {
    tx_hash: String,
    session_id: Option<String>,
    status_code: u16,
    response: Option<HttpResponse>,
}

/// Dispatch to charge or session payment flow.
async fn dispatch_payment(
    config: &Config,
    request_ctx: &RequestContext,
    output_opts: &OutputOptions,
    challenge_ctx: &ChallengeContext,
    url: &str,
    response: &HttpResponse,
) -> Result<PaymentResult> {
    if challenge_ctx.is_session {
        let result = handle_session_request(config, request_ctx, url, response).await?;
        match result {
            SessionResult::Streamed { channel_id } => Ok(PaymentResult {
                tx_hash: String::new(),
                session_id: Some(channel_id),
                status_code: 200,
                response: None,
            }),
            SessionResult::Response {
                response: resp,
                channel_id,
            } => Ok(PaymentResult {
                tx_hash: String::new(),
                session_id: Some(channel_id),
                status_code: resp.status_code,
                response: Some(resp),
            }),
        }
    } else {
        let auth_header = prepare_charge(config, &request_ctx.runtime, response).await?;

        if request_ctx.runtime.dry_run {
            eprintln!("[DRY RUN] Signed transaction ready, skipping submission.");
            return Ok(PaymentResult {
                tx_hash: String::new(),
                session_id: None,
                status_code: 200,
                response: None,
            });
        }

        if request_ctx.log_enabled() {
            eprintln!("Submitting payment...");
        }

        let headers = vec![("Authorization".to_string(), auth_header)];
        let resp = request_ctx.execute(url, Some(&headers)).await?;

        if resp.status_code >= 400 {
            return Err(parse_payment_rejection(&resp).into());
        }

        if request_ctx.log_enabled() {
            eprintln!("Payment accepted: HTTP {}", resp.status_code);
        }

        let network: Option<Network> = challenge_ctx.network.parse().ok();
        let explorer = network.and_then(|n| n.info().explorer);
        let (symbol, decimals) =
            resolve_token_meta(&challenge_ctx.network, &challenge_ctx.currency);
        display_receipt(
            output_opts,
            &resp,
            explorer.as_ref(),
            &challenge_ctx.amount,
            symbol,
            decimals,
        );

        // Extract a raw transaction reference (hex hash) for analytics if present
        let tx_hash = resp
            .get_header("payment-receipt")
            .and_then(|h| {
                mpp::protocol::core::extract_tx_hash(h)
                    .or_else(|| mpp::parse_receipt(h).ok().map(|r| r.reference))
            })
            .unwrap_or_default();
        let status_code = resp.status_code;
        Ok(PaymentResult {
            tx_hash,
            session_id: None,
            status_code,
            response: Some(resp),
        })
    }
}

/// Check if an error is due to missing config or an unprovisioned key —
/// i.e., something that `presto login` would fix.
fn is_login_fixable(err: &anyhow::Error) -> bool {
    err.chain().any(|e| {
        if let Some(pe) = e.downcast_ref::<PrestoError>() {
            matches!(
                pe,
                PrestoError::AccessKeyNotProvisioned | PrestoError::ConfigMissing(_)
            ) || matches!(pe, PrestoError::PaymentRejected { reason, .. }
                    if reason.contains("access key does not exist")
                       || reason.contains("access key is not provisioned"))
        } else {
            false
        }
    })
}

/// Parsed payment challenge context extracted from a 402 response.
struct ChallengeContext {
    is_session: bool,
    network: String,
    amount: String,
    currency: String,
}

/// Parse the WWW-Authenticate header from a 402 response and extract all
/// payment-related context needed for routing and analytics.
fn parse_payment_challenge(response: &HttpResponse) -> Result<ChallengeContext> {
    let www_auth = response
        .get_header("www-authenticate")
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

    let network_name = |chain_id: Option<u64>| -> String {
        chain_id
            .and_then(Network::from_chain_id)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    let (network, amount, currency) =
        if let Ok(charge) = challenge.request.decode::<mpp::ChargeRequest>() {
            (
                network_name(charge.chain_id()),
                charge.amount,
                charge.currency,
            )
        } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
            (
                network_name(session.chain_id()),
                session.amount,
                session.currency,
            )
        } else {
            ("unknown".to_string(), String::new(), String::new())
        };

    Ok(ChallengeContext {
        is_session,
        network,
        amount,
        currency,
    })
}

/// Ensure a wallet is available, prompting interactive login if needed.
async fn ensure_wallet_or_prompt_login(
    request_ctx: &RequestContext,
    cli: &Cli,
    config: &mut Config,
    analytics: &Option<Analytics>,
    challenge_network: &str,
) -> Result<()> {
    let has_wallet = WalletCredentials::load()
        .ok()
        .is_some_and(|c| c.has_wallet());

    if !has_wallet {
        if std::io::stdin().is_terminal() {
            eprintln!("This request requires payment. Let's connect your wallet first.\n");
            let network = request_ctx
                .runtime
                .network
                .as_deref()
                .or(Some(challenge_network));
            super::auth::run_login(network, analytics.clone(), super::OutputFormat::Text).await?;
            eprintln!("\nRetrying request...");
            *config = load_config_with_overrides(cli.config.as_ref())?;
        } else {
            anyhow::bail!(PrestoError::ConfigMissing(
                "No wallet configured.".to_string()
            ));
        }
    }

    Ok(())
}

/// Helper for tracking payment analytics without duplication.
///
/// Created once after parsing the 402 challenge, then used to track
/// started/success/failure events identically for both charge and session flows.
struct PaymentAnalytics {
    analytics: Option<Analytics>,
    network: String,
    amount: String,
    currency: String,
}

impl PaymentAnalytics {
    fn from_challenge(ctx: &ChallengeContext, analytics: &Option<Analytics>) -> Self {
        Self {
            analytics: analytics.clone(),
            network: ctx.network.clone(),
            amount: ctx.amount.clone(),
            currency: ctx.currency.clone(),
        }
    }

    fn track_started(&self) {
        if let Some(ref a) = self.analytics {
            a.track(
                analytics::Event::PaymentStarted,
                analytics::PaymentStartedPayload {
                    network: self.network.clone(),
                    amount: self.amount.clone(),
                    currency: self.currency.clone(),
                },
            );
        }
    }

    fn track_success(
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
                    tx_hash,
                    session_id,
                },
            );
            a.track(
                analytics::Event::QuerySuccess,
                analytics::QuerySuccessPayload {
                    url: analytics::sanitize_url(url),
                    method: method.to_string(),
                    status_code,
                },
            );
        }
    }

    fn track_failure(&self, err: &anyhow::Error) {
        if let Some(ref a) = self.analytics {
            a.track(
                analytics::Event::PaymentFailure,
                analytics::PaymentFailurePayload {
                    network: self.network.clone(),
                    amount: self.amount.clone(),
                    currency: self.currency.clone(),
                    error: analytics::sanitize_error(&err.to_string()),
                },
            );
        }
    }
}

/// Finalize a regular response: display output and fail on HTTP errors.
pub(crate) fn finalize_response(output_opts: &OutputOptions, response: HttpResponse) -> Result<()> {
    let status = response.status_code;
    handle_regular_response(output_opts, response)?;
    if status >= 400 {
        anyhow::bail!(PrestoError::Http(format!(
            "{} {}",
            status,
            http_status_text(status)
        )));
    }
    Ok(())
}

fn http_status_text(code: u16) -> &'static str {
    match code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

/// Parse a non-200 response after payment submission into a descriptive error.
fn parse_payment_rejection(response: &HttpResponse) -> PrestoError {
    let reason = if let Ok(body) = response.body_string() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
                error.to_string()
            } else if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
                message.to_string()
            } else if let Some(detail) = json.get("detail").and_then(|d| d.as_str()) {
                detail.to_string()
            } else {
                format!("HTTP {}", response.status_code)
            }
        } else if !body.trim().is_empty() {
            body.chars().take(200).collect()
        } else {
            format!("HTTP {}", response.status_code)
        }
    } else {
        format!("HTTP {}", response.status_code)
    };

    PrestoError::PaymentRejected {
        reason,
        status_code: response.status_code,
    }
}

/// Display receipt information from response with optional clickable explorer links.
fn display_receipt(
    output_opts: &OutputOptions,
    response: &HttpResponse,
    explorer: Option<&ExplorerConfig>,
    amount: &str,
    symbol: &str,
    decimals: u8,
) {
    // Always show payment summary when money moved (unless --quiet)
    if !output_opts.payment_log_enabled() {
        return;
    }

    // Format amount regardless of whether a receipt header is present
    let amount_display = amount
        .parse::<u128>()
        .ok()
        .map(|a| format_token_amount(a, symbol, decimals))
        .unwrap_or_else(|| format!("{} {}", amount, symbol));

    // Try to extract a transaction reference/link if the server provided a receipt header
    let mut link: Option<String> = None;
    let mut parsed_receipt: Option<mpp::Receipt> = None;
    if let Some(receipt_header) = response.get_header("payment-receipt") {
        // Prefer explicit tx hash; fall back to parsed reference
        let tx_ref = mpp::protocol::core::extract_tx_hash(receipt_header).or_else(|| {
            mpp::parse_receipt(receipt_header).ok().map(|r| {
                parsed_receipt = Some(r.clone());
                r.reference
            })
        });

        if let Some(tx) = tx_ref {
            let tx_link = if let Some(exp) = explorer {
                let url = exp.tx_url(&tx);
                hyperlink(&tx, &url)
            } else {
                tx
            };
            link = Some(tx_link);
        }
    }

    if let Some(l) = link {
        eprintln!("Paid {amount_display} · {l}");
    } else {
        eprintln!("Paid {amount_display}");
    }

    // Extended receipt details at -v (only if we successfully parsed the receipt)
    if output_opts.log_enabled() {
        if let Some(receipt) = parsed_receipt {
            eprintln!("  Status: {}", receipt.status);
            eprintln!("  Method: {}", receipt.method);
            eprintln!("  Timestamp: {}", receipt.timestamp);
        }
    }
}

// ==================== CLI → Domain Conversion ====================

/// Build a `RequestContext` from CLI arguments.
///
/// This is the boundary where CLI-specific types are converted into
/// domain types used by the HTTP and payment layers.
fn build_request_context(cli: &Cli, query: &QueryArgs) -> Result<RequestContext> {
    for header in &query.headers {
        validate_header_size(header)?;
        if header.contains('\r') || header.contains('\n') {
            anyhow::bail!(PrestoError::InvalidHeader(
                "header contains CR/LF characters".to_string()
            ));
        }
    }

    let runtime = RequestRuntime {
        verbosity: cli.verbosity(),
        show_output: cli.should_show_output(),
        network: cli.network.clone(),
        dry_run: query.dry_run,
    };

    // Determine method/body. If -I (HEAD) is provided, override method and ignore body inputs.
    let (method, body) = if query.head {
        get_request_method_and_body(Some("HEAD"), &[], None)?
    } else if query.method.is_some() {
        // Respect explicit -X even with -G; body still follows normal rules unless -G set
        if query.get {
            get_request_method_and_body(query.method.as_deref(), &[], None)?
        } else {
            get_request_method_and_body(
                query.method.as_deref(),
                &query.data,
                query.json.as_deref(),
            )?
        }
    } else if query.get {
        // Force GET with no body when -G is used without -X
        get_request_method_and_body(Some("GET"), &[], None)?
    } else {
        get_request_method_and_body(query.method.as_deref(), &query.data, query.json.as_deref())?
    };

    let mut headers = parse_headers(&query.headers);
    // Add Authorization: Basic ... if -u/--user provided and not explicitly overriden by -H
    if let Some(ref user) = query.user {
        if !crate::http::has_header(&query.headers, "authorization") {
            let encoded = base64::engine::general_purpose::STANDARD.encode(user);
            headers.push(("authorization".to_string(), format!("Basic {}", encoded)));
        }
    }
    // Add Authorization: Bearer if provided and not explicitly overridden
    if let Some(ref token) = query.bearer {
        if !crate::http::has_header(&query.headers, "authorization") && query.user.is_none() {
            headers.push(("authorization".to_string(), format!("Bearer {}", token)));
        }
    }
    // Add Referer header if provided and not overridden via -H
    if let Some(ref referer) = query.referer {
        if !crate::http::has_header(&query.headers, "referer") {
            headers.push(("referer".to_string(), referer.clone()));
        }
    }
    // Add Accept-Encoding on --compressed (reqwest negotiates automatically; header makes intent explicit)
    if query.compressed && !crate::http::has_header(&query.headers, "accept-encoding") {
        headers.push(("accept-encoding".to_string(), "gzip, br".to_string()));
    }
    if !query.head {
        if should_auto_add_json_content_type(&query.headers, query.json.as_deref(), &query.data) {
            headers.push(("content-type".to_string(), "application/json".to_string()));
        } else if !query.data_urlencode.is_empty()
            && !crate::http::has_header(&query.headers, "content-type")
        {
            headers.push((
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ));
        }
    }

    // If not using -G, merge --data-urlencode into body (form-encoded)
    let body = if !query.get && !query.data_urlencode.is_empty() {
        // Start with existing body bytes, then append &encoded
        let mut base = body.unwrap_or_default();
        let enc_pairs = parse_data_urlencode(&query.data_urlencode);
        let mut form = String::new();
        for (i, (name, val)) in enc_pairs.into_iter().enumerate() {
            if i > 0 {
                form.push('&');
            }
            if let Some(n) = name {
                form.push_str(&n);
                form.push('=');
                form.push_str(&val);
            } else {
                form.push_str(&val);
            }
        }
        if !base.is_empty() {
            base.push(b'&');
        }
        base.extend_from_slice(form.as_bytes());
        Some(base)
    } else {
        body
    };

    let plan = HttpRequestPlan {
        method,
        headers,
        body,
        timeout_secs: query.max_time,
        connect_timeout_secs: query.connect_timeout,
        retries: query.retries.unwrap_or(0),
        retry_backoff_ms: query.retry_backoff_ms.unwrap_or(250),
        follow_redirects: query.location,
        follow_redirects_limit: query.max_redirs.map(|v| v as usize),
        user_agent: query
            .user_agent
            .clone()
            .unwrap_or_else(|| format!("presto/{}", env!("CARGO_PKG_VERSION"))),
        insecure: query.insecure,
        proxy: query.proxy.clone(),
        no_proxy: query.no_proxy,
        http2: query.http2,
        http1_only: query.http1_1,
        verbose_connection: runtime.debug_enabled(),
    };

    Ok(RequestContext::new(runtime, plan))
}

/// Build `OutputOptions` from CLI arguments + config.
fn build_output_options(cli: &Cli, query: &QueryArgs, config: &Config) -> OutputOptions {
    OutputOptions {
        output_format: cli.resolve_output_format(config),
        // -I (HEAD) implies showing headers, even if -i wasn't explicitly set
        include_headers: query.include_headers || query.head,
        output_file: if query.output.is_none() && query.remote_name {
            // Derive a filename from the URL's last path segment; fallback to 'index.html'
            let url = &query.url;
            if let Ok(u) = url::Url::parse(url) {
                let seg = u
                    .path_segments()
                    .and_then(|mut s| s.next_back())
                    .filter(|v| !v.is_empty())
                    .unwrap_or("index.html");
                Some(seg.to_string())
            } else {
                None
            }
        } else {
            query.output.clone()
        },
        verbosity: cli.verbosity(),
        show_output: cli.should_show_output(),
        fail_silently: query.fail_silently && !query.fail_with_body,
        dump_headers: query.dump_header.clone(),
        write_meta: query.write_meta.clone(),
    }
}
