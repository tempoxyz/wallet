//! Integration tests for payment error handling
//!
//! Tests that payment errors produce user-friendly messages with actionable
//! suggestions and correct exit codes.
//!
//! Uses PRESTO_MOCK_PAYMENT env var to simulate payment failures without
//! requiring real wallet signing or RPC calls.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode};
use axum::{routing::any, Router};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

mod common;
use common::{setup_test_config, test_command};

struct TestServer {
    addr: std::net::SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _rt: Runtime,
}

impl TestServer {
    fn start_402_server() -> Self {
        let rt = Runtime::new().unwrap();
        let (addr, shutdown_tx) = rt.block_on(async {
            let app = Router::new().route(
                "/{path}",
                any(|| async {
                    (
                        StatusCode::PAYMENT_REQUIRED,
                        [(
                            "www-authenticate",
                            "Payment id=\"test\", realm=\"test\", method=\"tempo\", intent=\"charge\", request=\"e30\"",
                        )],
                        "",
                    )
                }),
            );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (tx, rx) = oneshot::channel::<()>();

            tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        rx.await.ok();
                    })
                    .await
                    .unwrap();
            });

            (addr, tx)
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            _rt: rt,
        }
    }

    fn start_rejection_server() -> Self {
        let rt = Runtime::new().unwrap();
        let (addr, shutdown_tx) = rt.block_on(async {
            let seen_auth = Arc::new(AtomicBool::new(false));

            let app = Router::new().route(
                "/{path}",
                any({
                    let seen_auth = seen_auth.clone();
                    move |headers: HeaderMap| {
                        let seen_auth = seen_auth.clone();
                        async move {
                            if headers.contains_key("authorization") {
                                seen_auth.store(true, Ordering::SeqCst);
                                (
                                    StatusCode::FORBIDDEN,
                                    [("content-type", "application/json")],
                                    r#"{"error":"insufficient_payment"}"#.to_string(),
                                )
                            } else {
                                (
                                    StatusCode::PAYMENT_REQUIRED,
                                    [(
                                        "www-authenticate",
                                        "Payment id=\"test\", realm=\"test\", method=\"tempo\", intent=\"charge\", request=\"e30\"",
                                    )],
                                    String::new(),
                                )
                            }
                        }
                    }
                }),
            );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (tx, rx) = oneshot::channel::<()>();

            tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        rx.await.ok();
                    })
                    .await
                    .unwrap();
            });

            (addr, tx)
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            _rt: rt,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.addr, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

#[test]
fn test_spending_limit_exceeded_error() {
    let server = TestServer::start_402_server();
    let temp = setup_test_config();

    let output = test_command(&temp)
        .env("PRESTO_MOCK_PAYMENT", "spending_limit_exceeded")
        .args(["query", &server.url("test")])
        .output()
        .expect("failed to run presto");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Spending limit exceeded"),
        "Expected spending limit error, got: {}",
        stderr
    );
    assert!(
        stderr.contains("Fix:"),
        "Expected Fix: label, got: {}",
        stderr
    );
    assert_eq!(
        output.status.code(),
        Some(6),
        "Expected exit code 6 (InsufficientFunds)"
    );
}

#[test]
fn test_insufficient_balance_error() {
    let server = TestServer::start_402_server();
    let temp = setup_test_config();

    let output = test_command(&temp)
        .env("PRESTO_MOCK_PAYMENT", "insufficient_balance")
        .args(["query", &server.url("test")])
        .output()
        .expect("failed to run presto");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Insufficient"),
        "Expected insufficient balance error, got: {}",
        stderr
    );
    assert!(
        stderr.contains("Fix:"),
        "Expected Fix: label, got: {}",
        stderr
    );
    assert_eq!(
        output.status.code(),
        Some(6),
        "Expected exit code 6 (InsufficientFunds)"
    );
}

#[test]
fn test_payment_rejected_error() {
    let server = TestServer::start_rejection_server();
    let temp = setup_test_config();

    let output = test_command(&temp)
        .env("PRESTO_MOCK_PAYMENT", "payment_rejected")
        .args(["query", &server.url("test")])
        .output()
        .expect("failed to run presto");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Payment rejected by server"),
        "Expected payment rejected error, got: {}",
        stderr
    );
    assert!(
        stderr.contains("insufficient_payment"),
        "Expected error reason from server, got: {}",
        stderr
    );
    assert_eq!(
        output.status.code(),
        Some(5),
        "Expected exit code 5 (PaymentFailed)"
    );
}
