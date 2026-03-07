//! Logout command — disconnect your wallet.

use crate::analytics::Event;
use crate::cli::Context;

#[derive(serde::Serialize)]
struct LogoutResponse {
    logged_in: bool,
    disconnected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub(crate) fn run(ctx: &Context, yes: bool) -> anyhow::Result<()> {
    let wallet_addr = match ctx.keys.find_passkey_wallet() {
        Some(entry) => entry.wallet_address.clone(),
        None => {
            if ctx.output_format.is_structured() {
                println!(
                    "{}",
                    ctx.output_format.serialize(&LogoutResponse {
                        logged_in: false,
                        disconnected: false,
                        wallet: None,
                        message: Some("not logged in".to_string()),
                    })?
                );
            } else {
                eprintln!("Not logged in.");
            }
            return Ok(());
        }
    };

    let short_addr = if wallet_addr.len() > 10 {
        format!(
            "{}...{}",
            &wallet_addr[..6],
            &wallet_addr[wallet_addr.len() - 4..]
        )
    } else {
        wallet_addr.to_string()
    };
    if !crate::util::confirm(&format!("Disconnect wallet {short_addr}?"), yes)? {
        if ctx.output_format.is_structured() {
            println!(
                "{}",
                ctx.output_format.serialize(&LogoutResponse {
                    logged_in: true,
                    disconnected: false,
                    wallet: Some(wallet_addr),
                    message: Some("cancelled".to_string()),
                })?
            );
        } else {
            eprintln!("Cancelled.");
        }
        return Ok(());
    }

    let mut keys = ctx.keys.clone();
    keys.delete_passkey_wallet(&wallet_addr)?;
    keys.save()?;

    if let Some(ref a) = ctx.analytics {
        a.track_event(Event::Logout);
    }

    if ctx.output_format.is_structured() {
        println!(
            "{}",
            ctx.output_format.serialize(&LogoutResponse {
                logged_in: true,
                disconnected: true,
                wallet: Some(wallet_addr),
                message: Some("wallet disconnected".to_string()),
            })?
        );
    } else {
        eprintln!("Wallet disconnected.");
    }
    Ok(())
}
