//! Logout command — disconnect your wallet.

use crate::analytics::{self, Event};
use crate::cli::Context;

pub(crate) async fn run(ctx: &Context, yes: bool) -> anyhow::Result<()> {
    let passkey_wallet_address = match ctx.keys.find_passkey_wallet() {
        Some(entry) => entry.wallet_address.clone(),
        None => {
            println!("Not logged in.");
            return Ok(());
        }
    };

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive logout");
        }

        let wallet_addr = &passkey_wallet_address;
        let short_addr = if wallet_addr.len() > 10 {
            format!(
                "{}...{}",
                &wallet_addr[..6],
                &wallet_addr[wallet_addr.len() - 4..]
            )
        } else {
            wallet_addr.to_string()
        };
        print!("Disconnect wallet {}? [y/N] ", short_addr);
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut keys = ctx.keys.clone();
    keys.delete_passkey_wallet(&passkey_wallet_address)?;
    keys.save()?;

    if let Some(ref a) = ctx.analytics {
        a.track(Event::Logout, analytics::EmptyPayload);
    }

    println!("Wallet disconnected.");
    Ok(())
}
