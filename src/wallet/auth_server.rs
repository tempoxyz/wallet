//! Local HTTP callback server for browser authentication.
//!
//! This module provides a temporary HTTP server that receives credentials
//! from the browser after passkey authentication. The callback returns a
//! `key_authorization` (hex-encoded `SignedKeyAuthorization` bytes) which
//! presto stores locally and includes in the first on-chain transaction to
//! atomically provision the access key and make a payment.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;
use tokio::sync::oneshot;
use tower_http::limit::RequestBodyLimitLayer;

use crate::error::{PrestoError, Result};

/// Credentials received from the browser callback.
///
/// The `key_authorization` field contains the hex-encoded RLP bytes of a
/// `SignedKeyAuthorization`. This is stored as a pending authorization and
/// included in the first on-chain transaction to atomically provision the
/// access key.
#[derive(Debug, Clone)]
pub struct AuthCallback {
    pub account_address: String,
    pub key_authorization: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackForm {
    account_address: String,
    key_authorization: Option<String>,
}

struct AppState {
    auth_server_base_url: String,
    tx: Option<oneshot::Sender<AuthCallback>>,
}

/// Start the local callback server.
///
/// Returns the port number and a receiver for the authentication callback.
pub async fn run_callback_server(
    auth_server_base_url: String,
) -> Result<(u16, oneshot::Receiver<AuthCallback>)> {
    let (tx, rx) = oneshot::channel();

    let state = Arc::new(tokio::sync::Mutex::new(AppState {
        auth_server_base_url,
        tx: Some(tx),
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to bind callback server: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| PrestoError::Http(format!("Failed to get local address: {}", e)))?
        .port();

    let app = Router::new()
        .route("/", get(health_check))
        .route("/callback", post(handle_callback))
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    Ok((port, rx))
}

async fn health_check() -> &'static str {
    "OK"
}

async fn handle_callback(
    State(state): State<Arc<tokio::sync::Mutex<AppState>>>,
    headers: HeaderMap,
    Form(form): Form<CallbackForm>,
) -> Response {
    let debug = std::env::var("PRESTO_DEBUG").is_ok();
    let mut state = state.lock().await;

    if debug {
        eprintln!("[presto:debug] Received auth callback");
        eprintln!("[presto:debug]   account_address: {}", form.account_address);
        eprintln!(
            "[presto:debug]   key_authorization: {:?}",
            form.key_authorization.as_ref().map(|s| format!(
                "{}...{}",
                &s[..std::cmp::min(10, s.len())],
                &s[s.len().saturating_sub(6)..]
            ))
        );
    }

    if !is_origin_allowed(&headers) {
        if debug {
            eprintln!(
                "[presto:debug] Origin rejected: {:?}",
                headers.get("origin").and_then(|v| v.to_str().ok())
            );
        }
        return (
            StatusCode::FORBIDDEN,
            Html(error_html("Request origin not allowed.")),
        )
            .into_response();
    }

    let callback = AuthCallback {
        account_address: form.account_address,
        key_authorization: form.key_authorization,
    };

    if debug {
        eprintln!("[presto:debug] Auth callback validated, saving credentials");
    }

    if let Some(tx) = state.tx.take() {
        let _ = tx.send(callback);
    }

    let success_url = format!("{}/cli-auth?success=true", state.auth_server_base_url);
    Redirect::to(&success_url).into_response()
}

const ALLOWED_ORIGIN_SUFFIX: &str = ".tempo.xyz";

fn is_origin_allowed(headers: &HeaderMap) -> bool {
    if let Ok(allowed) = std::env::var("PRESTO_ALLOW_ORIGIN") {
        let origin = headers.get("origin").and_then(|v| v.to_str().ok());
        return origin == Some(&allowed);
    }

    let origin = headers.get("origin").and_then(|v| v.to_str().ok());

    let origin = match origin {
        Some(o) if o != "null" => o,
        _ => return false,
    };

    let origin_url = match url::Url::parse(origin) {
        Ok(u) => u,
        Err(_) => return false,
    };

    let host = match origin_url.host_str() {
        Some(h) => h,
        None => return false,
    };

    if origin_url.scheme() != "https" {
        return false;
    }

    host == "tempo.xyz" || host.ends_with(ALLOWED_ORIGIN_SUFFIX)
}

fn error_html(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>presto - Error</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            color: white;
        }}
        .container {{
            text-align: center;
            padding: 2rem;
        }}
        .error {{
            font-size: 4rem;
            margin-bottom: 1rem;
        }}
        h1 {{
            margin: 0 0 0.5rem;
            font-size: 1.5rem;
            color: #ff6b6b;
        }}
        p {{
            color: #888;
            margin: 0;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="error">X</div>
        <h1>Authentication Error</h1>
        <p>{message}</p>
    </div>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_origin(origin: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("origin", origin.parse().unwrap());
        h
    }

    #[test]
    fn test_allows_tempo_xyz() {
        assert!(is_origin_allowed(&headers_with_origin("https://tempo.xyz")));
    }

    #[test]
    fn test_allows_subdomain() {
        assert!(is_origin_allowed(&headers_with_origin(
            "https://app.tempo.xyz"
        )));
        assert!(is_origin_allowed(&headers_with_origin(
            "https://app.moderato.tempo.xyz"
        )));
    }

    #[test]
    fn test_rejects_non_tempo_domain() {
        assert!(!is_origin_allowed(&headers_with_origin("https://evil.com")));
        assert!(!is_origin_allowed(&headers_with_origin(
            "https://nottempo.xyz"
        )));
        assert!(!is_origin_allowed(&headers_with_origin(
            "https://tempo.xyz.evil.com"
        )));
    }

    #[test]
    fn test_rejects_http() {
        assert!(!is_origin_allowed(&headers_with_origin(
            "http://app.tempo.xyz"
        )));
    }

    #[test]
    fn test_rejects_null_origin() {
        assert!(!is_origin_allowed(&headers_with_origin("null")));
    }

    #[test]
    fn test_rejects_missing_origin() {
        assert!(!is_origin_allowed(&HeaderMap::new()));
    }

    #[test]
    fn test_rejects_referer_without_origin() {
        let mut h = HeaderMap::new();
        h.insert("referer", "https://app.tempo.xyz/cli-auth".parse().unwrap());
        assert!(!is_origin_allowed(&h));
    }
}
