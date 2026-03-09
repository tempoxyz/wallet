//! Shared base context for extension CLIs.

use anyhow::Result;

use crate::analytics::{Analytics, Event, EventPayload};
use crate::config::Config;
use crate::keys::Keystore;
use crate::network::NetworkId;
use crate::util::Verbosity;

use super::output::OutputFormat;

/// Input parameters required to build the shared runtime context.
pub(crate) struct ContextArgs {
    pub(crate) config_path: Option<String>,
    pub(crate) rpc_url: Option<String>,
    pub(crate) requested_network: Option<String>,
    pub(crate) private_key: Option<String>,
    pub(crate) output_format: OutputFormat,
    pub(crate) verbosity: Verbosity,
}

/// Shared runtime context used by extension command handlers.
pub struct Context {
    pub config: Config,
    pub network: NetworkId,
    pub requested_network: Option<NetworkId>,
    pub keys: Keystore,
    pub analytics: Option<Analytics>,
    pub output_format: OutputFormat,
    pub verbosity: Verbosity,
}

impl Context {
    /// Track an analytics event with payload, no-op when telemetry is disabled.
    pub fn track<P: EventPayload>(&self, event: Event, payload: P) {
        if let Some(ref analytics) = self.analytics {
            analytics.track(event, payload);
        }
    }

    /// Track an analytics event without payload, no-op when telemetry is disabled.
    pub fn track_event(&self, event: Event) {
        if let Some(ref analytics) = self.analytics {
            analytics.track_event(event);
        }
    }

    /// Flush pending analytics events (with timeout).
    pub async fn flush_analytics(&self) {
        if let Some(ref a) = self.analytics {
            a.flush().await;
        }
    }

    /// Build the shared runtime context from parsed CLI arguments.
    pub(crate) async fn build(args: ContextArgs) -> Result<Self> {
        let config = Config::load(args.config_path.as_ref(), args.rpc_url.as_deref())?;
        let requested_network = args
            .requested_network
            .as_deref()
            .map(|network| NetworkId::resolve(Some(network)))
            .transpose()?;
        let network = NetworkId::resolve(args.requested_network.as_deref())?;
        let keys = Keystore::load(args.private_key.as_deref())?;
        let analytics = Analytics::new(network, &config, &keys).await;

        Ok(Self {
            config,
            network,
            requested_network,
            keys,
            analytics,
            output_format: args.output_format,
            verbosity: args.verbosity,
        })
    }
}
