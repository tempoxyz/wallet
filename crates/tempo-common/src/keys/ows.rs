//! OWS (Open Wallet Standard) — encrypted key storage.
//!
//! Uses the `ows-lib` crate directly for wallet operations.

use zeroize::Zeroizing;

use crate::error::{ConfigError, TempoError};

/// Create a new OWS wallet and return its UUID.
pub fn create_wallet(name: &str) -> Result<String, TempoError> {
    let wallet = ows_lib::create_wallet(name, Some(12), None, None)
        .map_err(|e| ows_error("create_wallet", e))?;
    Ok(wallet.id)
}

/// Decrypt and return the EVM signing key from an OWS wallet.
pub fn export_private_key(name_or_id: &str) -> Result<Zeroizing<String>, TempoError> {
    let key_bytes = ows_lib::decrypt_signing_key(
        name_or_id,
        ows_core::ChainType::Evm,
        "",
        None,
        None,
    )
    .map_err(|e| ows_error("decrypt_signing_key", e))?;

    let hex = format!("0x{}", hex::encode(key_bytes.expose()));
    Ok(Zeroizing::new(hex))
}

fn ows_error(op: &str, e: ows_lib::OwsLibError) -> TempoError {
    TempoError::Config(ConfigError::Missing(format!("OWS {op}: {e}")))
}
