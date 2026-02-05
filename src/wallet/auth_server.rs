//! Local HTTP callback server for browser authentication.
//!
//! This module provides a temporary HTTP server that receives credentials
//! from the browser after passkey authentication.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;
use tokio::sync::oneshot;
use tower_http::limit::RequestBodyLimitLayer;

use crate::error::{PgetError, Result};

/// Credentials received from the browser callback.
#[derive(Debug, Clone)]
pub struct AuthCallback {
    pub access_key: String,
    pub account_address: String,
    #[allow(dead_code)]
    pub key_id: String,
    pub expiry: u64,
    #[allow(dead_code)]
    pub tx_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackForm {
    access_key: String,
    account_address: String,
    key_id: String,
    expiry: String,
    tx_hash: Option<String>,
    state: String,
}

struct AppState {
    expected_state: String,
    auth_server_base_url: String,
    tx: Option<oneshot::Sender<AuthCallback>>,
}

/// Start the local callback server.
///
/// Returns the port number and a receiver for the authentication callback.
pub async fn run_callback_server(
    expected_state: String,
    auth_server_base_url: String,
) -> Result<(u16, oneshot::Receiver<AuthCallback>)> {
    let (tx, rx) = oneshot::channel();

    let state = Arc::new(tokio::sync::Mutex::new(AppState {
        expected_state,
        auth_server_base_url,
        tx: Some(tx),
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| PgetError::Http(format!("Failed to bind callback server: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| PgetError::Http(format!("Failed to get local address: {}", e)))?
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
    Form(form): Form<CallbackForm>,
) -> Response {
    let debug = std::env::var("PGET_DEBUG").is_ok();
    let mut state = state.lock().await;

    if debug {
        eprintln!("[pget:debug] Received auth callback");
        eprintln!("[pget:debug]   account_address: {}", form.account_address);
        eprintln!("[pget:debug]   key_id: {}", form.key_id);
        eprintln!("[pget:debug]   expiry: {}", form.expiry);
        eprintln!("[pget:debug]   tx_hash: {:?}", form.tx_hash);
        eprintln!(
            "[pget:debug]   access_key: {}...{}",
            &form.access_key[..std::cmp::min(10, form.access_key.len())],
            &form.access_key[form.access_key.len().saturating_sub(6)..]
        );
    }

    // Validate CSRF state token
    if form.state != state.expected_state {
        if debug {
            eprintln!("[pget:debug] CSRF state mismatch!");
        }
        return (
            StatusCode::BAD_REQUEST,
            Html(error_html(
                "Invalid state token. Please restart the authentication process.",
            )),
        )
            .into_response();
    }

    let callback = AuthCallback {
        access_key: form.access_key,
        account_address: form.account_address,
        key_id: form.key_id,
        expiry: form.expiry.parse().unwrap_or(0),
        tx_hash: form.tx_hash,
    };

    if debug {
        eprintln!("[pget:debug] Auth callback validated, saving credentials");
    }

    if let Some(tx) = state.tx.take() {
        let _ = tx.send(callback);
    }

    let success_url = format!("{}/cli-auth?success=true", state.auth_server_base_url);
    Redirect::to(&success_url).into_response()
}

fn error_html(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>pget - Error</title>
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
