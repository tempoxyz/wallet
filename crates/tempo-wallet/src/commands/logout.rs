//! Logout command — disconnect your wallet.

use alloy::primitives::Address;

use crate::analytics::LOGOUT;
use tempo_common::cli::context::Context;
use tempo_common::cli::output;
use tempo_common::error::{ConfigError, TempoError};
use tempo_common::keys::Keystore;

#[derive(serde::Serialize)]
struct LogoutResponse {
    logged_in: bool,
    disconnected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub(crate) fn run(ctx: &Context, yes: bool) -> Result<(), TempoError> {
    let (wallet_addr, wallet_address) = if let Some(wallet) = resolve_passkey_wallet(&ctx.keys)? {
        wallet
    } else {
        output::emit_by_format(
            ctx.output_format,
            &LogoutResponse {
                logged_in: false,
                disconnected: false,
                wallet: None,
                message: Some("not logged in".to_string()),
            },
            || {
                eprintln!("Not logged in.");
                Ok(())
            },
        )?;
        return Ok(());
    };

    let short_addr = if wallet_addr.len() > 10 {
        format!(
            "{}...{}",
            &wallet_addr[..6],
            &wallet_addr[wallet_addr.len() - 4..]
        )
    } else {
        wallet_addr.clone()
    };
    if !crate::prompt::confirm(&format!("Disconnect wallet {short_addr}?"), yes)? {
        output::emit_by_format(
            ctx.output_format,
            &LogoutResponse {
                logged_in: true,
                disconnected: false,
                wallet: Some(wallet_addr),
                message: Some("cancelled".to_string()),
            },
            || {
                eprintln!("Cancelled.");
                Ok(())
            },
        )?;
        return Ok(());
    }

    let mut keys = ctx.keys.clone();
    keys.delete_passkey_wallet_address(wallet_address)?;
    keys.save()?;

    ctx.track_event(LOGOUT);

    output::emit_by_format(
        ctx.output_format,
        &LogoutResponse {
            logged_in: true,
            disconnected: true,
            wallet: Some(wallet_addr),
            message: Some("wallet disconnected".to_string()),
        },
        || {
            eprintln!("Wallet disconnected.");
            Ok(())
        },
    )?;
    Ok(())
}

fn resolve_passkey_wallet(keys: &Keystore) -> Result<Option<(String, Address)>, ConfigError> {
    let Some(entry) = keys.find_passkey_wallet() else {
        return Ok(None);
    };

    parse_passkey_wallet_entry(entry).map(Some)
}

fn parse_passkey_wallet_entry(
    entry: &tempo_common::keys::KeyEntry,
) -> Result<(String, Address), ConfigError> {
    let wallet_address =
        entry
            .wallet_address_parsed()
            .ok_or_else(|| ConfigError::InvalidAddress {
                context: "stored passkey wallet",
                value: entry.wallet_address.clone(),
            })?;
    let wallet_addr = format!("{wallet_address:#x}");

    Ok((wallet_addr, wallet_address))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_common::keys::{KeyEntry, WalletType};

    #[test]
    fn parse_passkey_wallet_entry_rejects_malformed_address() {
        let entry = KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "not-an-address".to_string(),
            ..Default::default()
        };

        let err = parse_passkey_wallet_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("invalid"));
    }
}
