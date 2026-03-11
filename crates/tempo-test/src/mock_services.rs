//! Mock MPP service directory server.

use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

/// Mock MPP service directory server.
pub struct MockServicesServer {
    pub services_url: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockServicesServer {
    /// Start a mock services directory with a default payload.
    pub async fn start() -> Self {
        Self::start_with_payload(json!({
            "services": [
                {
                    "id": "openai",
                    "name": "OpenAI",
                    "url": "https://openrouter.mpp.tempo.xyz",
                    "serviceUrl": "https://openrouter.mpp.tempo.xyz/v1/chat/completions",
                    "description": "LLM API",
                    "categories": ["ai"],
                    "methods": {"tempo": {"intents": ["charge"]}}
                }
            ]
        }))
        .await
    }

    /// Start a mock services directory with a custom payload.
    pub async fn start_with_payload(payload: serde_json::Value) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let services_url = format!("http://{addr}/services");

        let app = Router::new().route(
            "/services",
            get(move || {
                let p = payload.clone();
                async move { Json(p) }
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

        MockServicesServer {
            services_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }
}

impl Drop for MockServicesServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}
