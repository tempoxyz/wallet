//! Opt-out telemetry via PostHog.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::sync::{Arc, Mutex};

use posthog_rs::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::config::Config;
use crate::keys::Keystore;
use crate::network::NetworkId;

// ---------------------------------------------------------------------------
// PostHog client
// ---------------------------------------------------------------------------

const POSTHOG_API_KEY: &str = "phc_aNlTw2xAUQKd9zTovXeYheEUpQpEhplehCK5r1e31HR";

async fn ph_capture(client: &Client, distinct_id: &str, event_name: &str, properties: Value) {
    let mut event = posthog_rs::Event::new(event_name, distinct_id);
    let _ = event.insert_prop("tempo_app_id", "tempo-wallet");
    let _ = event.insert_prop("$lib", "tempo-wallet");
    let _ = event.insert_prop("$lib_version", env!("CARGO_PKG_VERSION"));
    let _ = event.insert_prop("os", std::env::consts::OS);
    let _ = event.insert_prop("arch", std::env::consts::ARCH);

    if let Value::Object(map) = properties {
        for (k, v) in map {
            let _ = event.insert_prop(k, v);
        }
    }

    let _ = client.capture(event).await;
}

async fn ph_alias(client: &Client, previous_id: &str, new_id: &str) {
    let mut event = posthog_rs::Event::new("$create_alias", new_id);
    let _ = event.insert_prop("alias", previous_id);
    let _ = event.insert_prop("tempo_app_id", "tempo-wallet");

    let _ = client.capture(event).await;
}

async fn ph_identify(client: &Client, distinct_id: &str, properties: Value) {
    let mut event = posthog_rs::Event::new("$identify", distinct_id);
    let _ = event.insert_prop("tempo_app_id", "tempo-wallet");
    let _ = event.insert_prop("$set", properties);

    let _ = client.capture(event).await;
}

// ---------------------------------------------------------------------------
// Analytics client
// ---------------------------------------------------------------------------

fn is_telemetry_disabled(config: &Config) -> bool {
    if std::env::var("TEMPO_NO_TELEMETRY").is_ok() {
        return true;
    }

    !config.telemetry.enabled
}

/// Generate a stable anonymous ID unique to the OS user on this machine.
///
/// Note: `DefaultHasher` output is not guaranteed stable across Rust versions,
/// so anonymous IDs may change on compiler upgrades. This is acceptable for
/// analytics — the user will simply appear as a new anonymous user.
fn anonymous_id() -> String {
    let mut hasher = DefaultHasher::new();
    let mut has_input = false;
    if let Ok(name) = hostname::get() {
        name.hash(&mut hasher);
        has_input = true;
    }
    // Include the OS username so different users on the same host get distinct IDs
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        user.hash(&mut hasher);
        has_input = true;
    }
    // Fallback: use the config directory path as a stable per-user identifier
    // (avoids all container users collapsing to the same anonymous ID)
    if !has_input {
        if let Some(dir) = dirs::config_dir() {
            dir.hash(&mut hasher);
        }
    }
    format!("anon-{:016x}", hasher.finish())
}

#[derive(Clone)]
pub struct Analytics {
    client: Arc<Client>,
    distinct_id: String,
    network: String,
    pending: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Analytics {
    pub async fn new(network: NetworkId, config: &Config, keys: &Keystore) -> Option<Self> {
        if is_telemetry_disabled(config) {
            return None;
        }

        let client = posthog_rs::client(POSTHOG_API_KEY).await;
        let addr = keys.wallet_address();
        let distinct_id = if addr.is_empty() {
            anonymous_id()
        } else {
            addr.to_string()
        };

        let analytics = Self {
            client: Arc::new(client),
            distinct_id,
            network: network.to_string(),
            pending: Arc::new(Mutex::new(Vec::new())),
        };
        analytics.identify(keys);
        Some(analytics)
    }

    pub fn track_event(&self, event: Event) {
        self.track(event, EmptyPayload);
    }

    pub fn track<P: EventPayload>(&self, event: Event, payload: P) {
        let client = self.client.clone();
        let distinct_id = self.distinct_id.clone();
        let event_name = event.as_str().to_string();
        let network = self.network.clone();

        let mut props = serde_json::to_value(&payload).unwrap_or_default();
        if let Value::Object(ref mut map) = props {
            map.entry("network".to_string()).or_insert(json!(network));
        }

        // Test hook: if TEMPO_TEST_EVENTS is set, append events to the file for assertions
        test_tap_event(&event_name, &props);

        let handle = tokio::spawn(async move {
            ph_capture(&client, &distinct_id, &event_name, props).await;
        });
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(handle);
        }
    }

    pub fn identify(&self, keys: &Keystore) {
        let addr = keys.wallet_address();
        if addr.is_empty() {
            return;
        }
        let wallet = addr.to_string();

        let client = self.client.clone();
        let old_id = self.distinct_id.clone();
        let network = self.network.clone();

        let handle = tokio::spawn(async move {
            if old_id != wallet {
                ph_alias(&client, &old_id, &wallet).await;
            }
            ph_identify(
                &client,
                &wallet,
                json!({
                    "wallet_address": &wallet,
                    "cli_version": env!("CARGO_PKG_VERSION"),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "network": network,
                }),
            )
            .await;
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
    if let Ok(path) = std::env::var("TEMPO_TEST_EVENTS") {
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

// ---------------------------------------------------------------------------
// Events and payloads
// ---------------------------------------------------------------------------

/// Analytics event identifier.
///
/// A thin wrapper around a static string. Each crate defines its own
/// domain-specific event constants; shared lifecycle events live in
/// the [`events`] module below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event(&'static str);

impl Event {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

/// Shared lifecycle events used by CLI runner infrastructure.
pub mod events {
    use super::Event;

    pub const COMMAND_RUN: Event = Event::new("command_run");
    pub const COMMAND_SUCCESS: Event = Event::new("command_success");
    pub const COMMAND_FAILURE: Event = Event::new("command_failure");
    pub const COOP_CLOSE_SUCCESS: Event = Event::new("coop_close_success");
    pub const COOP_CLOSE_FAILURE: Event = Event::new("coop_close_failure");
}

/// Trait for analytics event payloads.
///
/// The `'static` bound is required because payloads are moved into `tokio::spawn` tasks.
pub trait EventPayload: Serialize + Send + Sync + 'static {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EmptyPayload;
impl EventPayload for EmptyPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandRunPayload {
    pub(crate) command: String,
}
impl EventPayload for CommandRunPayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandFailurePayload {
    pub(crate) command: String,
    pub(crate) error: String,
}
impl EventPayload for CommandFailurePayload {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CoopClosePayload {
    pub(crate) network: String,
    pub(crate) channel_id: String,
}
impl EventPayload for CoopClosePayload {}
