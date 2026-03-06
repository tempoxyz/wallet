//! Logout command — disconnect your wallet.

use crate::analytics::Event;
use crate::cli::Context;

pub(crate) fn run(ctx: &Context, yes: bool) -> anyhow::Result<()> {
    let wallet_addr = match ctx.keys.find_passkey_wallet() {
        Some(entry) => entry.wallet_address.clone(),
        None => {
            eprintln!("Not logged in.");
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
        eprintln!("Cancelled.");
        return Ok(());
    }

    let mut keys = ctx.keys.clone();
    keys.delete_passkey_wallet(&wallet_addr)?;
    keys.save()?;

    if let Some(ref a) = ctx.analytics {
        a.track_event(Event::Logout);
    }

    eprintln!("Wallet disconnected.");
    Ok(())
}
