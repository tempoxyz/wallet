//! Mock HTTP servers for integration tests.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;

/// Generic mock HTTP server backed by axum.
pub struct MockServer {
    pub base_url: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockServer {
    /// Start a server that always returns the given status, headers, and body.
    pub async fn start(status: u16, headers: Vec<(&str, &str)>, body: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let status_code = StatusCode::from_u16(status).unwrap();
        let owned_headers: Vec<(String, String)> = headers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let owned_body = body.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move || {
                let hdrs = owned_headers.clone();
                let b = owned_body.clone();
                async move {
                    let mut response = (status_code, b).into_response();
                    for (k, v) in &hdrs {
                        response.headers_mut().insert(
                            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                            axum::http::HeaderValue::from_str(v).unwrap(),
                        );
                    }
                    response
                }
            }),
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a payment mock: returns 402 + WWW-Authenticate when no Authorization
    /// header is present, returns 200 + body when Authorization header is present.
    pub async fn start_payment(www_authenticate: &str, success_body: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let owned_header = www_authenticate.to_string();
        let owned_body = success_body.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move |headers: axum::http::HeaderMap| {
                let h = owned_header.clone();
                let b = owned_body.clone();
                async move {
                    if headers.get("authorization").is_some() {
                        (StatusCode::OK, b).into_response()
                    } else {
                        let mut response =
                            (StatusCode::PAYMENT_REQUIRED, "Payment Required").into_response();
                        response.headers_mut().insert(
                            axum::http::HeaderName::from_static("www-authenticate"),
                            axum::http::HeaderValue::from_str(&h).unwrap(),
                        );
                        response
                    }
                }
            }),
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a payment mock that also returns a Payment-Receipt header on success.
    pub async fn start_payment_with_receipt(
        www_authenticate: &str,
        success_body: &str,
        receipt_header: &str,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let owned_header = www_authenticate.to_string();
        let owned_body = success_body.to_string();
        let owned_receipt = receipt_header.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move |headers: axum::http::HeaderMap| {
                let h = owned_header.clone();
                let b = owned_body.clone();
                let r = owned_receipt.clone();
                async move {
                    if headers.get("authorization").is_some() {
                        let mut resp = (StatusCode::OK, b).into_response();
                        resp.headers_mut().insert(
                            axum::http::HeaderName::from_static("payment-receipt"),
                            axum::http::HeaderValue::from_str(&r).unwrap(),
                        );
                        resp
                    } else {
                        let mut response =
                            (StatusCode::PAYMENT_REQUIRED, "Payment Required").into_response();
                        response.headers_mut().insert(
                            axum::http::HeaderName::from_static("www-authenticate"),
                            axum::http::HeaderValue::from_str(&h).unwrap(),
                        );
                        response
                    }
                }
            }),
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a mock that echoes request headers back as a JSON body.
    pub async fn start_echo_headers() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let app = Router::new().route(
            "/{*path}",
            any(move |headers: axum::http::HeaderMap| async move {
                let mut map = serde_json::Map::new();
                for (k, v) in headers.iter() {
                    if let Ok(s) = v.to_str() {
                        map.insert(
                            k.as_str().to_string(),
                            serde_json::Value::String(s.to_string()),
                        );
                    }
                }
                let body = serde_json::to_string(&map).unwrap();
                (StatusCode::OK, body).into_response()
            }),
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a mock that echoes back the full request as JSON:
    /// `{ "method": "...", "path": "...", "query": "...", "headers": {...}, "body": "..." }`
    pub async fn start_echo_request() -> Self {
        use axum::http::Request;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let app = Router::new().route(
            "/{*path}",
            any(move |req: Request<axum::body::Body>| async move {
                let method = req.method().to_string();
                let path = req.uri().path().to_string();
                let query = req.uri().query().unwrap_or("").to_string();
                let mut hdr_map = serde_json::Map::new();
                for (k, v) in req.headers().iter() {
                    if let Ok(s) = v.to_str() {
                        hdr_map.insert(
                            k.as_str().to_string(),
                            serde_json::Value::String(s.to_string()),
                        );
                    }
                }
                let body_bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024)
                    .await
                    .unwrap_or_default();
                let body_str = String::from_utf8_lossy(&body_bytes).to_string();
                let obj = serde_json::json!({
                    "method": method,
                    "path": path,
                    "query": query,
                    "headers": hdr_map,
                    "body": body_str,
                });
                (StatusCode::OK, serde_json::to_string(&obj).unwrap()).into_response()
            }),
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a mock that returns an SSE stream with the given raw body.
    pub async fn start_sse(body: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let owned_body = body.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move || {
                let b = owned_body.clone();
                async move {
                    let mut response = (StatusCode::OK, b).into_response();
                    response.headers_mut().insert(
                        axum::http::HeaderName::from_static("content-type"),
                        axum::http::HeaderValue::from_static("text/event-stream"),
                    );
                    response
                }
            }),
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Get the full URL for a path on this server.
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

// ── Payment challenge helpers ───────────────────────────────────────────

/// Base64url-no-padding of canonical JSON for a Moderato charge challenge
/// (1 USDC to Hardhat #1, chain 42431).
pub const MODERATO_CHARGE_CHALLENGE: &str = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";

/// Build a WWW-Authenticate header for a Moderato charge challenge.
pub fn charge_www_authenticate(id: &str) -> String {
    format!(
        r#"Payment id="{id}", realm="mock", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
    )
}
