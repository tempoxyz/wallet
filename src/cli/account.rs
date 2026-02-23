//! Account management commands — list, switch, rename, delete profiles.

use anyhow::Result;

use crate::wallet::credentials::WalletCredentials;

/// List all account profiles.
pub fn list_accounts() -> Result<()> {
    let creds = WalletCredentials::load()?;

    if creds.accounts.is_empty() {
        println!("No accounts. Run 'presto login' to add one.");
        return Ok(());
    }

    // Print active first, then remaining profiles sorted lexicographically
    let mut names: Vec<String> = creds.accounts.keys().cloned().collect();
    names.sort();

    if !creds.active.is_empty() && creds.accounts.contains_key(&creds.active) {
        let name = &creds.active;
        let account = creds.accounts.get(name).unwrap();
        let addr = if account.account_address.is_empty() {
            "(no address)"
        } else {
            &account.account_address
        };
        println!("  {name} *  {addr}");
    }

    for name in names {
        if name == creds.active {
            continue;
        }
        let account = &creds.accounts[&name];
        let addr = if account.account_address.is_empty() {
            "(no address)"
        } else {
            &account.account_address
        };
        println!("  {name}    {addr}");
    }

    Ok(())
}

/// Switch the active profile.
pub fn switch_account(profile: &str) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    creds.switch(profile)?;
    creds.save()?;
    println!("Switched to profile '{profile}'.");
    Ok(())
}

/// Rename a profile.
pub fn rename_account(old: &str, new: &str) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    creds.rename_account(old, new)?;
    creds.save()?;
    println!("Renamed profile '{old}' to '{new}'.");
    Ok(())
}

/// Delete a profile.
pub fn delete_account(profile: &str, yes: bool) -> Result<()> {
    let creds = WalletCredentials::load()?;

    if !creds.accounts.contains_key(profile) {
        anyhow::bail!("Profile '{profile}' not found.");
    }

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive delete");
        }

        print!("Delete profile '{profile}'? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut creds = creds;
    creds.delete_account(profile)?;
    creds.save()?;
    println!("Deleted profile '{profile}'.");
    Ok(())
}
