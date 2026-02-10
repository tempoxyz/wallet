use anyhow::Result;

use crate::payment::providers::stream::StreamState;
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

    let stream_state = StreamState::load().ok();
    let open_channels = stream_state.as_ref().map(|s| s.channels.len()).unwrap_or(0);

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive logout");
        }

        if open_channels > 0 {
            println!(
                "Warning: {} open stream channel(s) will be cleared.",
                open_channels
            );
            println!("Run `tempoctl stream close --all` first to recover deposits.");
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

    if open_channels > 0 {
        let mut state = stream_state.unwrap();
        state.channels.clear();
        state.save()?;
        println!("Cleared {} stream channel(s).", open_channels);
    }

    creds.clear_wallet();
    creds.save()?;
    println!("Wallet disconnected.");
    Ok(())
}
