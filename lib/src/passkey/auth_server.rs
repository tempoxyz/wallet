//! Local HTTP server to receive OAuth-style callbacks from presto.tempo.xyz.

use crate::error::{PurlError, Result};
use serde::Deserialize;
use std::io::Read;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server};

#[derive(Debug, Deserialize)]
pub struct CallbackPayload {
    pub access_key: String,
    pub account_address: String,
    pub key_id: String,
    pub expiry: u64,
    pub public_key: String,
    pub state: String,
}

pub struct AuthServer {
    port: u16,
    csrf_token: String,
}

impl AuthServer {
    pub fn new() -> Result<Self> {
        // NOTE: There is a small TOCTOU race between dropping this listener and binding
        // the tiny_http server. This is low-risk since we bind to localhost only and the
        // window is very short. tiny_http::Server doesn't support creating from an existing
        // TcpListener, so we accept this minor race condition.
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);

        let csrf_token = generate_csrf_token();

        Ok(Self { port, csrf_token })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn csrf_token(&self) -> &str {
        &self.csrf_token
    }

    pub fn presto_url(&self, network: &str) -> String {
        format!(
            "https://presto.tempo.xyz/auth?network={}&callback=http://127.0.0.1:{}/callback&state={}",
            network, self.port, self.csrf_token
        )
    }

    pub async fn wait_for_callback(&self, timeout_secs: u64) -> Result<CallbackPayload> {
        let (tx, rx) = mpsc::channel();
        let port = self.port;
        let expected_state = self.csrf_token.clone();
        let timeout = Duration::from_secs(timeout_secs);

        thread::spawn(move || {
            let server = match Server::http(format!("127.0.0.1:{}", port)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(PurlError::Http(format!(
                        "Failed to start auth server: {}",
                        e
                    ))));
                    return;
                }
            };

            loop {
                match server.recv_timeout(timeout) {
                    Ok(Some(request)) => {
                        if request.url().starts_with("/callback") {
                            let result = handle_callback_request(request, &expected_state);
                            let _ = tx.send(result);
                            return;
                        } else {
                            let response = Response::from_string("Not Found").with_status_code(404);
                            let _ = request.respond(response);
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(Err(PurlError::Http("Auth server timeout".to_string())));
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(PurlError::Http(format!("Auth server error: {}", e))));
                        return;
                    }
                }
            }
        });

        rx.recv_timeout(Duration::from_secs(timeout_secs + 5))
            .map_err(|_| PurlError::Http("Auth callback timeout".to_string()))?
    }
}

const MAX_BODY_SIZE: u64 = 32 * 1024; // 32KB

fn handle_callback_request(
    mut request: tiny_http::Request,
    expected_state: &str,
) -> Result<CallbackPayload> {
    if request.method() != &tiny_http::Method::Post {
        let response = Response::from_string("Method Not Allowed").with_status_code(405);
        let _ = request.respond(response);
        return Err(PurlError::Http("Expected POST request".to_string()));
    }

    let content_length = request.body_length().unwrap_or(0);
    if content_length > MAX_BODY_SIZE as usize {
        let response = Response::from_string("Payload Too Large").with_status_code(413);
        let _ = request.respond(response);
        return Err(PurlError::Http("Request body too large".to_string()));
    }

    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY_SIZE)
        .read_to_string(&mut body)?;

    let payload: CallbackPayload = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            let response = Response::from_string("Bad Request").with_status_code(400);
            let _ = request.respond(response);
            return Err(PurlError::Http(format!("Invalid callback payload: {}", e)));
        }
    };

    if payload.state != expected_state {
        let response = Response::from_string("Invalid state parameter").with_status_code(400);
        let _ = request.respond(response);
        return Err(PurlError::Http("CSRF token mismatch".to_string()));
    }

    let html = r#"<!DOCTYPE html>
<html>
<head><title>Authentication Complete</title></head>
<body style="font-family: sans-serif; text-align: center; padding: 50px;">
<h1>Authentication Successful</h1>
<p>You can close this window and return to the terminal.</p>
</body>
</html>"#;

    let header =
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
    let response = Response::from_string(html)
        .with_status_code(200)
        .with_header(header);
    let _ = request.respond(response);

    Ok(payload)
}

fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_server_new() {
        let server = AuthServer::new().unwrap();
        assert!(server.port() > 0);
        assert_eq!(server.csrf_token().len(), 64);
    }

    #[test]
    fn test_presto_url() {
        let server = AuthServer::new().unwrap();
        let url = server.presto_url("tempo-testnet");
        assert!(url.starts_with("https://presto.tempo.xyz/auth?"));
        assert!(url.contains("network=tempo-testnet"));
        assert!(url.contains(&format!("callback=http://127.0.0.1:{}", server.port())));
        assert!(url.contains(&format!("state={}", server.csrf_token())));
    }

    #[test]
    fn test_generate_csrf_token() {
        let token1 = generate_csrf_token();
        let token2 = generate_csrf_token();
        assert_eq!(token1.len(), 64);
        assert_ne!(token1, token2);
    }
}
