//! Opt-out telemetry via `PostHog`.

use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use posthog_rs::Client;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::time::{timeout, Duration};

use crate::{config::Config, keys::Keystore, network::NetworkId};

// ---------------------------------------------------------------------------
// PostHog client
// ---------------------------------------------------------------------------

const POSTHOG_API_KEY: &str = "phc_aNlTw2xAUQKd9zTovXeYheEUpQpEhplehCK5r1e31HR";

fn build_event(
    event_name: &str,
    distinct_id: &str,
    app_id: &str,
    session_id: &str,
    properties: Value,
) -> posthog_rs::Event {
    let mut event = posthog_rs::Event::new(event_name, distinct_id);
    let _ = event.insert_prop("tempo_app_id", app_id);
    let _ = event.insert_prop("$lib", app_id);
    let _ = event.insert_prop("$lib_version", env!("CARGO_PKG_VERSION"));
    let _ = event.insert_prop("os", std::env::consts::OS);
    let _ = event.insert_prop("arch", std::env::consts::ARCH);
    let _ = event.insert_prop("$session_id", session_id);

    if let Value::Object(map) = properties {
        for (k, v) in map {
            let _ = event.insert_prop(k, v);
        }
    }

    event
}

fn build_alias_event(app_id: &str, previous_id: &str, new_id: &str) -> posthog_rs::Event {
    let mut event = posthog_rs::Event::new("$create_alias", new_id);
    let _ = event.insert_prop("alias", previous_id);
    let _ = event.insert_prop("tempo_app_id", app_id);
    event
}

fn build_identify_event(
    app_id: &str,
    distinct_id: &str,
    set_props: Value,
    set_once_props: Value,
) -> posthog_rs::Event {
    let mut event = posthog_rs::Event::new("$identify", distinct_id);
    let _ = event.insert_prop("tempo_app_id", app_id);
    let _ = event.insert_prop("$set", set_props);
    let _ = event.insert_prop("$set_once", set_once_props);
    event
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
/// Uses SHA-256 for a deterministic hash that is stable across Rust compiler
/// versions (unlike `DefaultHasher`).
fn anonymous_id() -> String {
    let mut hasher = Sha256::new();
    let has_host_input = hostname::get().is_ok_and(|name| {
        hasher.update(name.as_encoded_bytes());
        true
    });
    // Include the OS username so different users on the same host get distinct IDs
    let has_user_input = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .is_ok_and(|user| {
            hasher.update(user.as_bytes());
            true
        });
    let has_input = has_host_input || has_user_input;
    // Fallback: use the config directory path as a stable per-user identifier
    // (avoids all container users collapsing to the same anonymous ID)
    if !has_input {
        if let Some(dir) = dirs::config_dir() {
            hasher.update(dir.to_string_lossy().as_bytes());
        }
    }
    let hash = hasher.finalize();
    format!("anon-{}", hex::encode(&hash[..8]))
}

/// Generate a per-invocation session ID (UUIDv4-like from random bytes).
fn generate_session_id() -> String {
    let mut bytes = [0u8; 16];
    // Best-effort: fall back to zeros if randomness is unavailable
    let _ = getrandom::getrandom(&mut bytes);
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&bytes[0..4]),
        hex::encode(&bytes[4..6]),
        hex::encode(&bytes[6..8]),
        hex::encode(&bytes[8..10]),
        hex::encode(&bytes[10..16]),
    )
}

#[derive(Clone)]
pub struct Analytics {
    client: Arc<Client>,
    distinct_id: String,
    app_id: &'static str,
    session_id: String,
    network: String,
    pending: Arc<Mutex<Vec<posthog_rs::Event>>>,
}

impl Analytics {
    pub async fn new(
        network: NetworkId,
        config: &Config,
        keys: &Keystore,
        app_id: &'static str,
    ) -> Option<Self> {
        if is_telemetry_disabled(config) {
            return None;
        }

        let client = posthog_rs::client(POSTHOG_API_KEY).await;
        let distinct_id = keys.wallet_address_hex().unwrap_or_else(anonymous_id);

        let analytics = Self {
            client: Arc::new(client),
            distinct_id,
            app_id,
            session_id: generate_session_id(),
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
        let event_name = event.as_str();

        let mut props = serde_json::to_value(&payload).unwrap_or_default();
        if let Value::Object(ref mut map) = props {
            map.entry("network".to_string())
                .or_insert(json!(self.network));
        }

        // Test hook: if TEMPO_TEST_EVENTS is set, append events to the file for assertions
        test_tap_event(event_name, &props);

        let event = build_event(
            event_name,
            &self.distinct_id,
            self.app_id,
            &self.session_id,
            props,
        );

        if let Ok(mut pending) = self.pending.lock() {
            pending.push(event);
        }
    }

    pub fn identify(&self, keys: &Keystore) {
        let Some(wallet) = keys.wallet_address_hex() else {
            return;
        };

        if self.distinct_id != wallet {
            let alias_event = build_alias_event(self.app_id, &self.distinct_id, &wallet);
            if let Ok(mut pending) = self.pending.lock() {
                pending.push(alias_event);
            }
        }

        let identify_event = build_identify_event(
            self.app_id,
            &wallet,
            json!({
                "wallet_address": &wallet,
                "cli_version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "network": &self.network,
            }),
            json!({
                "first_cli_version": env!("CARGO_PKG_VERSION"),
                "first_seen_at": OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
            }),
        );
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(identify_event);
        }
    }

    pub async fn flush(&self) {
        let events = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *pending)
        };

        if events.is_empty() {
            return;
        }

        let client = self.client.clone();
        let _ = timeout(Duration::from_secs(2), async move {
            let _ = client.capture_batch(events, false).await;
        })
        .await;
    }
}

fn test_tap_event(name: &str, props: &Value) {
    if let Ok(path) = std::env::var("TEMPO_TEST_EVENTS") {
        if path.is_empty() {
            return;
        }
        let line = format!("{name}|{props}\n");
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
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

/// Shared lifecycle events used by CLI runner infrastructure.
pub mod events {
    use super::Event;

    pub const COMMAND_SUCCESS: Event = Event::new("command succeeded");
    pub const COMMAND_FAILURE: Event = Event::new("command failed");
    pub const COOP_CLOSE_SUCCESS: Event = Event::new("coop close succeeded");
    pub const COOP_CLOSE_FAILURE: Event = Event::new("coop close failed");
    pub const SESSION_STORE_DEGRADED: Event = Event::new("session store degraded");
    pub const KEYSTORE_LOAD_DEGRADED: Event = Event::new("keystore load degraded");
}

/// Marker trait for analytics event payloads.
///
/// Automatically implemented for any type that is `Serialize + Send + Sync + 'static`.
pub trait EventPayload: Serialize + Send + Sync + 'static {}
impl<T: Serialize + Send + Sync + 'static> EventPayload for T {}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EmptyPayload;

#[derive(Debug, Clone, Serialize)]
pub struct CommandSuccessPayload {
    pub command: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandFailurePayload {
    pub command: String,
    pub error: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CoopClosePayload {
    pub(crate) network: String,
    pub(crate) channel_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionStoreDegradedPayload {
    pub(crate) malformed_load_drops: u64,
    pub(crate) malformed_list_drops: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct KeystoreLoadDegradedPayload {
    pub(crate) strict_parse_failures: u64,
    pub(crate) salvage_malformed_entries: u64,
    pub(crate) filtered_invalid_entries: u64,
}
