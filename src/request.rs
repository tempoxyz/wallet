//! Request orchestration: query → 402 detection → payment → response handling.
//!
//! This module owns the main HTTP request flow, including automatic payment
//! when the server responds with 402 Payment Required. It coordinates between
//! the charge and session payment paths, handles wallet login prompting,
//! and tracks analytics throughout the lifecycle.

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

    ensure_wallet_or_prompt_login(&request_ctx, &mut config, &analytics).await?;

    let challenge_ctx = parse_payment_challenge(&response)?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Payment protocol: {}", challenge_ctx.protocol);
    }

    let pay_analytics = PaymentAnalytics::from_challenge(&challenge_ctx, &analytics);
    pay_analytics.track_started();

    if challenge_ctx.is_session {
        if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
            eprintln!("Payment intent: session");
        }

        match handle_session_request(&config, &request_ctx, &url, &response).await {
            Ok(result) => {
                pay_analytics.track_success(String::new(), &url, &method_str, 200);
                match result {
                    SessionResult::Streamed => Ok(()),
                    SessionResult::Response(resp) => {
                        finalize_response(&request_ctx.cli, &request_ctx.query, resp)
                    }
                }
            }
            Err(e) => {
                pay_analytics.track_failure(&e);
                Err(e)
            }
        }
    } else {
        match handle_charge_request(&config, &request_ctx, &url, &response).await {
            Ok(response) => {
                let tx_hash = response
                    .get_header("payment-receipt")
                    .cloned()
                    .unwrap_or_default();
                let status_code = response.status_code;
                pay_analytics.track_success(tx_hash, &url, &method_str, status_code);
                finalize_response(&request_ctx.cli, &request_ctx.query, response)?;
                Ok(())
            }
            Err(e) => {
                pay_analytics.track_failure(&e);
                Err(e)
            }
        }
    }
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
            let net = crate::payment::mpp_ext::network_from_charge_request(&charge)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            (net, charge.amount, charge.currency)
        } else if let Ok(session) = challenge.request.decode::<mpp::SessionRequest>() {
            let net = crate::payment::mpp_ext::network_from_session_request(&session)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            (net, session.amount, session.currency)
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
    let has_wallet = config.evm.is_some()
        || crate::wallet::credentials::WalletCredentials::load()
            .ok()
            .and_then(|c| c.active_wallet().cloned())
            .is_some();

    if !has_wallet && std::env::var("PRESTO_MOCK_PAYMENT").is_err() {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            eprintln!("This request requires payment. Let's connect your wallet first.\n");
            let network = request_ctx.cli.network.as_deref();
            crate::cli::commands::login::run_login(network, analytics.clone()).await?;
            eprintln!("\nRetrying request...");
            *config = load_config_with_overrides(&request_ctx.cli)?;
        } else {
            anyhow::bail!(crate::error::PrestoError::ConfigMissing(
                "No wallet configured. Run 'presto login' to connect your wallet, then retry the request.".to_string()
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
