mod events;
mod posthog;

pub use events::*;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::config::Config;
use crate::wallet::credentials::WalletCredentials;

fn is_telemetry_disabled(config: Option<&Config>) -> bool {
    if std::env::var("PRESTO_NO_TELEMETRY").is_ok() {
        return true;
    }
    // Honor the config that was already loaded by the caller (respects --config)
    if let Some(cfg) = config {
        return !cfg.telemetry.enabled;
    }
    false
}

/// Result of loading wallet credentials at analytics init time.
struct WalletInfo {
    address: Option<String>,
    has_wallet: bool,
}

fn load_wallet_info() -> WalletInfo {
    match WalletCredentials::load() {
        Ok(creds) => {
            let addr = creds.wallet_address();
            WalletInfo {
                address: if addr.is_empty() {
                    None
                } else {
                    Some(addr.to_string())
                },
                has_wallet: creds.has_wallet(),
            }
        }
        Err(_) => WalletInfo {
            address: None,
            has_wallet: false,
        },
    }
}

/// Generate a stable anonymous ID unique to the OS user on this machine.
fn anonymous_id() -> String {
    let mut hasher = DefaultHasher::new();
    if let Ok(name) = hostname::get() {
        name.hash(&mut hasher);
    }
    // Include the OS username so different users on the same host get distinct IDs
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        user.hash(&mut hasher);
    }
    format!("anon-{:016x}", hasher.finish())
}

#[derive(Clone)]
pub struct Analytics {
    client: Arc<posthog_rs::Client>,
    distinct_id: String,
    wallet_address: Option<String>,
    has_wallet: bool,
    network: Option<String>,
    pending: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Analytics {
    pub async fn new(network: Option<&str>, config: Option<&Config>) -> Option<Self> {
        if is_telemetry_disabled(config) {
            return None;
        }

        let client = posthog::build_client().await?;
        let wallet_info = load_wallet_info();
        let distinct_id = wallet_info.address.clone().unwrap_or_else(anonymous_id);

        Some(Self {
            client: Arc::new(client),
            distinct_id,
            wallet_address: wallet_info.address,
            has_wallet: wallet_info.has_wallet,
            network: network.map(|s| s.to_string()),
            pending: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Whether a fully configured wallet was found when analytics was initialized.
    pub fn has_wallet(&self) -> bool {
        self.has_wallet
    }

    pub fn track<P: EventPayload>(&self, event: Event, payload: P) {
        let client = self.client.clone();
        let distinct_id = self.distinct_id.clone();
        let event_name = event.as_str().to_string();
        let network = self.network.clone();

        let mut props = serde_json::to_value(&payload).unwrap_or_default();
        if let Some(ref net) = network {
            if let Value::Object(ref mut map) = props {
                map.entry("network".to_string()).or_insert(json!(net));
            }
        }

        // Test hook: if PRESTO_TEST_EVENTS is set, append events to the file for assertions
        test_tap_event(&event_name, &props);

        let handle = tokio::spawn(async move {
            posthog::capture(&client, &distinct_id, &event_name, props).await;
        });
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(handle);
        }
    }

    pub fn identify(&self) {
        let Some(wallet) = self.wallet_address.clone() else {
            return;
        };

        let client = self.client.clone();
        let old_id = self.distinct_id.clone();
        let network = self.network.clone();

        let handle = tokio::spawn(async move {
            let identify_fut = posthog::identify(
                &client,
                &wallet,
                json!({
                    "wallet_address": &wallet,
                    "cli_version": env!("CARGO_PKG_VERSION"),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "network": network,
                }),
            );
            if old_id != wallet {
                tokio::join!(posthog::alias(&client, &old_id, &wallet), identify_fut);
            } else {
                identify_fut.await;
            }
        });
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(handle);
        }
    }

    pub async fn flush(&self) {
        let handles = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *pending)
        };

        let _ = timeout(Duration::from_secs(2), futures::future::join_all(handles)).await;
    }
}

fn test_tap_event(name: &str, props: &Value) {
    if let Ok(path) = std::env::var("PRESTO_TEST_EVENTS") {
        if path.is_empty() {
            return;
        }
        let line = format!("{}|{}\n", name, props);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
