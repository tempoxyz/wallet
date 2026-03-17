//! Session management commands.

mod close;
mod list;
mod render;
mod sync;
mod util;

use crate::args::SessionCommands;
use tempo_common::{
    cli::context::Context,
    error::{ConfigError, TempoError},
};

// Common imports shared by submodules
use tempo_common::session::{self, ChannelStatus};

pub(crate) async fn run(ctx: &Context, command: SessionCommands) -> Result<(), TempoError> {
    match command {
        SessionCommands::List { orphaned, all } => {
            require_wallet_login(ctx)?;
            list::list_channels(ctx, orphaned, all).await
        }
        SessionCommands::Close {
            url,
            all,
            orphaned,
            finalize,
            cooperative,
            dry_run,
        } => close::close_sessions(ctx, url, all, orphaned, finalize, cooperative, dry_run).await,
        SessionCommands::Sync { origin } => {
            require_wallet_login(ctx)?;
            sync::sync_sessions(ctx, origin.as_deref()).await
        }
    }
}

fn require_wallet_login(ctx: &Context) -> Result<(), TempoError> {
    if ctx.keys.has_wallet() {
        Ok(())
    } else {
        Err(ConfigError::Missing(
            "No wallet configured. Log in with 'tempo wallet login'.".to_string(),
        )
        .into())
    }
}
