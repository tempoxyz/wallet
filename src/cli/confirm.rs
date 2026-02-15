//! Payment confirmation dialogs for interactive terminal sessions.
//!
//! This module provides user confirmation prompts for payment operations,
//! displaying payment details in a formatted box before requesting confirmation.

use anyhow::{Context, Result};
use dialoguer::Confirm;
use std::str::FromStr;

use crate::config::{Config, WalletConfig};
use crate::network::explorer::ExplorerConfig;
use crate::network::Network;
use crate::payment::mpp_ext::method_to_network;
use mpp::{ChargeRequest, PaymentChallenge};

use super::exit_codes::ExitCode;
use super::formatting::{format_truncated_address_link, pad_with_hyperlink};

/// Confirm payment with user for web payments.
///
/// Displays a formatted payment details box and prompts the user to confirm.
/// Returns an error if not running in an interactive terminal.
/// Exits with `UserCancelled` if the user declines.
pub fn confirm_web_payment(
    config: &Config,
    challenge: &PaymentChallenge,
    charge_req: &ChargeRequest,
    explorer: Option<&ExplorerConfig>,
) -> Result<()> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Cannot confirm payment: not running in an interactive terminal.\n\
             Remove --confirm flag or run in an interactive terminal."
        );
    }

    let network_name = method_to_network(&challenge.method)
        .ok_or_else(|| anyhow::anyhow!("Unsupported payment method: {}", challenge.method))?;

    let from_address = config
        .require_evm()
        .and_then(|evm| evm.get_address())
        .unwrap_or_else(|_| "unknown".to_string());

    let network = Network::from_str(network_name)
        .map_err(|e| anyhow::anyhow!("Unknown network '{}': {}", network_name, e))?;
    let token_config = network
        .require_token_config(&charge_req.currency)
        .context("Cannot display formatted payment amount")?;
    let (decimals, symbol) = (token_config.currency.decimals, token_config.currency.symbol);

    let amount_u128: u128 = charge_req.amount.parse().unwrap_or(0);
    let divisor = 10u128.pow(decimals as u32) as f64;
    let amount_display = format!("{:.6} {}", amount_u128 as f64 / divisor, symbol);

    let asset_display = format_truncated_address_link(&charge_req.currency, 45, explorer);
    let to_display = format_truncated_address_link(
        charge_req.recipient.as_deref().unwrap_or("(server)"),
        45,
        explorer,
    );
    let from_display = format_truncated_address_link(&from_address, 45, explorer);

    eprintln!();
    eprintln!("┌─────────────────────────────────────────────────────────────┐");
    eprintln!("│                      Payment Details                        │");
    eprintln!("├─────────────────────────────────────────────────────────────┤");
    eprintln!("│  Amount:    {:<47} │", amount_display);
    eprintln!("│  Asset:     {} │", pad_with_hyperlink(&asset_display, 47));
    eprintln!("│  Network:   {:<47} │", network_name);
    eprintln!("│  To:        {} │", pad_with_hyperlink(&to_display, 47));
    eprintln!("│  From:      {} │", pad_with_hyperlink(&from_display, 47));
    eprintln!("└─────────────────────────────────────────────────────────────┘");
    eprintln!();

    let confirm = Confirm::new()
        .with_prompt("Proceed with this payment?")
        .default(false)
        .interact()?;

    if !confirm {
        ExitCode::UserCancelled.exit();
    }

    Ok(())
}
