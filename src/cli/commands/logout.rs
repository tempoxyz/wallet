//! Wallet logout command implementation.

use anyhow::Result;

use crate::wallet::credentials::WalletCredentials;

pub async fn run_logout(yes: bool, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    if creds.active_wallet().is_none() {
        println!("No wallet connected.");
        return Ok(());
    }

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive logout");
        }

        print!("Disconnect wallet? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    creds.clear_wallet();
    creds.save()?;
    println!("Wallet disconnected.");
    Ok(())
}
