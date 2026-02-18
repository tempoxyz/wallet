//! Wallet management: signers and Tempo passkey wallets.

use alloy::rlp::Decodable;
use tempo_primitives::transaction::SignedKeyAuthorization;

pub mod access_key;
pub mod credentials;
mod device_code;
mod manager;
mod pkce;
pub mod signer;

pub use access_key::AccessKey;
pub use manager::WalletManager;

/// Decode a hex-encoded SignedKeyAuthorization.
///
/// Accepts hex strings with or without a "0x" prefix.
/// Logs a warning if the input is present but fails to decode.
pub fn decode_key_authorization(hex_str: &str) -> Option<SignedKeyAuthorization> {
    let raw = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = match hex::decode(raw) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Invalid pending key authorization hex: {e}");
            return None;
        }
    };
    let mut slice = bytes.as_slice();
    match SignedKeyAuthorization::decode(&mut slice) {
        Ok(auth) => Some(auth),
        Err(e) => {
            tracing::warn!("Invalid pending key authorization RLP: {e}");
            None
        }
    }
}
