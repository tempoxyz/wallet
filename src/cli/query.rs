//! Query command: HTTP request with automatic payment handling.
//!
//! This module implements the `query` command — the primary CLI flow.
//! It sends the initial HTTP request, detects 402 Payment Required responses,
//! dispatches to the charge or session payment path, handles wallet login
//! prompting, and displays the final response.

use std::io::IsTerminal;

use anyhow::{Context, Result};
use mpp::protocol::methods::tempo::session::TempoSessionExt;
use mpp::protocol::methods::tempo::TempoChargeExt;
use mpp::PaymentProtocol;

use crate::analytics::{self, Analytics};
use crate::config::{load_config_with_overrides, Config};
use crate::error::PrestoError;
use crate::http::{
    get_request_method_and_body, parse_headers, should_auto_add_json_content_type,
    validate_header_size, HttpRequestPlan, HttpResponse, RequestContext, RequestRuntime,
};
use crate::network::{resolve_token_meta, ExplorerConfig, Network};
use crate::payment::charge::prepare_charge;
use crate::payment::session::{handle_session_request, SessionResult};
use crate::util::{format_token_amount, hyperlink};
use crate::wallet::credentials::{has_credentials_override, WalletCredentials};

use super::output::{handle_regular_response, OutputOptions};
use super::{Cli, QueryArgs};

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
    // The  TEMPO_RPC_URLenv var is already handled by load_config_with_overrides,
    // but the explicit --rpc flag takes final precedence.
    if let Some(ref rpc_url) = query.rpc_url {
        config.set_rpc_override(rpc_url.clone());
    }

    let url = query.url.clone();

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

    let response = match request_ctx.execute(&url, None).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::QueryFailure,
                    analytics::QueryFailurePayload {
                        url: sanitized_url,
                        method: method_str,
                        error: analytics::sanitize_error(&e.to_string()),
                    },
                );
            }
            return Err(e);
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
                finalize_response(&output_opts, resp)?;
            }
            Ok(())
        }
        Err(e) => {
            pay_analytics.track_failure(&e);
            Err(e)
        }
    }
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
/// i.e., something that ` tempo-walletlogin` would fix.
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

    let (method, body) =
        get_request_method_and_body(query.method.as_deref(), &query.data, query.json.as_deref())?;

    let mut headers = parse_headers(&query.headers);
    if should_auto_add_json_content_type(&query.headers, query.json.as_deref(), &query.data) {
        headers.push(("content-type".to_string(), "application/json".to_string()));
    }

    let plan = HttpRequestPlan {
        method,
        headers,
        body,
        timeout_secs: query.max_time,
        follow_redirects: !query.no_redirect,
        user_agent: format!("presto/{}", env!("CARGO_PKG_VERSION")),
        verbose_connection: runtime.debug_enabled(),
    };

    Ok(RequestContext::new(runtime, plan))
}

/// Build `OutputOptions` from CLI arguments + config.
fn build_output_options(cli: &Cli, query: &QueryArgs, config: &Config) -> OutputOptions {
    OutputOptions {
        output_format: cli.resolve_output_format(config),
        include_headers: query.include_headers,
        output_file: query.output.clone(),
        verbosity: cli.verbosity(),
        show_output: cli.should_show_output(),
    }
}
