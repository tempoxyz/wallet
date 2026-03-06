//! Shared application context: construction and configuration.

use anyhow::Result;

use super::output::OutputFormat;
use super::Cli;
use crate::analytics::Analytics;
use crate::config::Config;
use crate::keys::Keystore;
use crate::network::NetworkId;
use crate::version;

/// Shared application context built once in [`Cli::run`] and threaded
/// to all command handlers.
pub(crate) struct Context {
    pub(in crate::cli) cli: Cli,
    pub(in crate::cli) config: Config,
    pub(in crate::cli) network: NetworkId,
    pub(in crate::cli) keys: Keystore,
    pub(in crate::cli) analytics: Option<Analytics>,
    pub(in crate::cli) output_format: OutputFormat,
}

impl Context {
    /// Build the shared application context from CLI args.
    pub(super) async fn build(cli: Cli) -> Result<Self> {
        let mut config = Config::load(cli.config.as_ref(), cli.rpc_url.as_deref())?;
        version::check_for_updates(&mut config).await;

        let network = NetworkId::resolve(cli.network.as_deref())?;
        let keys = Keystore::load(cli.private_key.as_deref())?;
        let output_format = cli.resolve_output_format();
        let analytics = Analytics::new(network, &config, &keys).await;

        Ok(Self {
            cli,
            config,
            network,
            keys,
            analytics,
            output_format,
        })
    }
}
