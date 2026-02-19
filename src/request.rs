//! Request orchestration: query → 402 detection → payment → response handling.
//!
//! This module owns the main HTTP request flow, including automatic payment
//! when the server responds with 402 Payment Required. It coordinates between
//! the charge and session payment paths, handles wallet login prompting,
//! and tracks analytics throughout the lifecycle.

use std::io::IsTerminal;

use anyhow::{Context, Result};
use mpp::PaymentProtocol;

use crate::analytics::{self, Analytics};
use crate::cli::output::handle_regular_response;
use crate::cli::{Cli, QueryArgs};
use crate::config::{load_config_with_overrides, Config};
use crate::http::request::RequestContext;
use crate::http::HttpResponse;
use crate::payment::charge::handle_charge_request;
use crate::payment::session::{handle_session_request, SessionResult};

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
    let mut config = load_config_with_overrides(&cli)?;

    // Apply --rpc flag override to config.
    // The PRESTO_RPC_URL env var is already handled by load_config_with_overrides,
    // but the explicit --rpc flag takes final precedence.
    if let Some(ref rpc_url) = query.rpc_url {
        config.set_rpc_override(rpc_url.clone());
    }

    let url = query.url.clone();
    let request_ctx = RequestContext::new(cli, query)?;
    let method_str = request_ctx.method.to_string();

    if let Some(ref a) = analytics {
        a.track(
            analytics::Event::QueryStarted,
            analytics::QueryStartedPayload {
                url: url.clone(),
                method: method_str.clone(),
            },
        );
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Making {} request to: {url}", request_ctx.method);
    }

    let response = match request_ctx.execute(&url, None).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::QueryFailure,
                    analytics::QueryFailurePayload {
                        url: url.clone(),
                        method: method_str,
                        error: e.to_string(),
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
                    url: url.clone(),
                    method: method_str,
                    status_code: response.status_code,
                },
            );
        }
        finalize_response(&request_ctx.cli, &request_ctx.query, response)?;
        return Ok(());
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("402 status: payment required");
    }

    let challenge_ctx = parse_payment_challenge(&response)?;

    // Skip wallet login for dry-run — the user just wants to see what would happen
    if !request_ctx.query.dry_run {
        ensure_wallet_or_prompt_login(&request_ctx, &mut config, &analytics).await?;
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Payment protocol: {}", challenge_ctx.protocol);
    }

    let pay_analytics = PaymentAnalytics::from_challenge(&challenge_ctx, &analytics);
    pay_analytics.track_started();

    let result = dispatch_payment(&config, &request_ctx, &challenge_ctx, &url, &response).await;

    // Auto-login retry: if the key isn't provisioned and we're interactive, login and retry once
    let result = match result {
        Err(e) if is_not_provisioned(&e) && std::io::stdin().is_terminal() => {
            eprintln!("Access key is not provisioned on-chain. Running login to set it up...\n");
            let network = request_ctx
                .cli
                .network
                .as_deref()
                .or(Some(challenge_ctx.network.as_str()));
            crate::cli::auth::run_login(network, analytics.clone()).await?;
            eprintln!("\nRetrying payment...");

            let config = load_config_with_overrides(&request_ctx.cli)?;
            dispatch_payment(&config, &request_ctx, &challenge_ctx, &url, &response).await
        }
        other => other,
    };

    match result {
        Ok(result) => {
            mark_network_provisioned(&challenge_ctx.network);
            pay_analytics.track_success(result.tx_hash, &url, &method_str, result.status_code);
            if let Some(resp) = result.response {
                finalize_response(&request_ctx.cli, &request_ctx.query, resp)?;
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
    status_code: u32,
    response: Option<HttpResponse>,
}

/// Dispatch to charge or session payment flow.
async fn dispatch_payment(
    config: &Config,
    request_ctx: &RequestContext,
    challenge_ctx: &ChallengeContext,
    url: &str,
    response: &HttpResponse,
) -> Result<PaymentResult> {
    if challenge_ctx.is_session {
        if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
            eprintln!("Payment intent: session");
        }
        let result = handle_session_request(config, request_ctx, url, response).await?;
        match result {
            SessionResult::Streamed => Ok(PaymentResult {
                tx_hash: String::new(),
                status_code: 200,
                response: None,
            }),
            SessionResult::Response(resp) => Ok(PaymentResult {
                tx_hash: String::new(),
                status_code: resp.status_code,
                response: Some(resp),
            }),
        }
    } else {
        let resp = handle_charge_request(config, request_ctx, url, response).await?;
        let tx_hash = resp
            .get_header("payment-receipt")
            .cloned()
            .unwrap_or_default();
        let status_code = resp.status_code;
        Ok(PaymentResult {
            tx_hash,
            status_code,
            response: Some(resp),
        })
    }
}

/// Check if an error is due to an unprovisioned access key.
fn is_not_provisioned(err: &anyhow::Error) -> bool {
    err.chain().any(|e| {
        if let Some(pe) = e.downcast_ref::<crate::error::PrestoError>() {
            matches!(pe, crate::error::PrestoError::AccessKeyNotProvisioned)
                || matches!(pe, crate::error::PrestoError::PaymentRejected { reason, .. }
                    if reason.contains("access key does not exist")
                       || reason.contains("access key is not provisioned"))
        } else {
            false
        }
    })
}

/// Mark a network as provisioned in wallet.toml after a successful payment.
fn mark_network_provisioned(network: &str) {
    crate::wallet::credentials::WalletCredentials::mark_provisioned(network);
}

/// Parsed payment challenge context extracted from a 402 response.
struct ChallengeContext {
    protocol: PaymentProtocol,
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
        .ok_or_else(|| crate::error::PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let protocol = PaymentProtocol::detect(Some(www_auth.as_str())).ok_or_else(|| {
        crate::error::PrestoError::MissingHeader("WWW-Authenticate: Payment".to_string())
    })?;

    let challenge =
        mpp::parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    let is_session = challenge.intent.is_session();

    let (network, amount, currency) =
        if let Ok(charge) = challenge.request.decode::<mpp::ChargeRequest>() {
            use mpp::protocol::methods::tempo::TempoChargeExt;
            let name = charge.chain_id()
                .and_then(crate::network::Network::from_chain_id)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            (name, charge.amount, charge.currency)
        } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
            use mpp::protocol::methods::tempo::session::TempoSessionExt;
            let name = session.chain_id()
                .and_then(crate::network::Network::from_chain_id)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            (name, session.amount, session.currency)
        } else {
            ("unknown".to_string(), String::new(), String::new())
        };

    Ok(ChallengeContext {
        protocol,
        is_session,
        network,
        amount,
        currency,
    })
}

/// Ensure a wallet is available, prompting interactive login if needed.
async fn ensure_wallet_or_prompt_login(
    request_ctx: &RequestContext,
    config: &mut Config,
    analytics: &Option<Analytics>,
) -> Result<()> {
    let has_wallet = crate::wallet::credentials::WalletCredentials::load()
        .ok()
        .is_some_and(|c| c.has_wallet());

    if !has_wallet {
        if std::io::stdin().is_terminal() {
            eprintln!("This request requires payment. Let's connect your wallet first.\n");
            let network = request_ctx.cli.network.as_deref();
            crate::cli::auth::run_login(network, analytics.clone()).await?;
            eprintln!("\nRetrying request...");
            *config = load_config_with_overrides(&request_ctx.cli)?;
        } else {
            anyhow::bail!(crate::error::PrestoError::ConfigMissing(
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

    fn track_success(&self, tx_hash: String, url: &str, method: &str, status_code: u32) {
        if let Some(ref a) = self.analytics {
            a.track(
                analytics::Event::PaymentSuccess,
                analytics::PaymentSuccessPayload {
                    network: self.network.clone(),
                    amount: self.amount.clone(),
                    currency: self.currency.clone(),
                    tx_hash,
                },
            );
            a.track(
                analytics::Event::QuerySuccess,
                analytics::QuerySuccessPayload {
                    url: url.to_string(),
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
                    error: err.to_string(),
                },
            );
        }
    }
}

/// Finalize a regular response: display output and fail on HTTP errors.
pub(crate) fn finalize_response(
    cli: &Cli,
    query: &QueryArgs,
    response: HttpResponse,
) -> Result<()> {
    let status = response.status_code;
    handle_regular_response(cli, query, response)?;
    if status >= 400 {
        anyhow::bail!(crate::error::PrestoError::Http(format!(
            "{} {}",
            status,
            http_status_text(status)
        )));
    }
    Ok(())
}

fn http_status_text(code: u32) -> &'static str {
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
