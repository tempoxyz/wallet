//! Refresh command — force re-authentication to renew the current access key.

use tempo_common::{cli::context::Context, error::TempoError};

use crate::commands::login;

pub(crate) async fn run(
    ctx: &Context,
    limit: Option<u64>,
    expiry: Option<u64>,
    token: Option<String>,
) -> Result<(), TempoError> {
    login::run_with_reauth(ctx, limit, expiry, token).await
}
