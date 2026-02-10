use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::{Address, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use anyhow::{bail, Context, Result};
use tempo_primitives::transaction::Call;
use tracing::info;

use crate::config::load_config;
use crate::network::Network;
use crate::payment::abi::{encode_escrow_request_close, encode_escrow_withdraw};
use crate::payment::providers::stream::{query_on_chain_channel, StreamState};
use crate::payment::providers::tempo::create_tempo_transaction_with_calls;
use crate::wallet::signer::load_signer_with_priority;

const GRACE_PERIOD_SECS: u64 = 900;

pub fn list_channels() -> Result<()> {
    let state = StreamState::load()?;

    if state.channels.is_empty() {
        println!("No open stream channels.");
        return Ok(());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    println!("Open stream channels:\n");
    for (i, (_key, entry)) in state.channels.iter().enumerate() {
        println!("  [{}] {}", i, entry.channel_id);
        println!("      Payee:       {}", entry.payee);
        println!("      Token:       {}", entry.token);
        println!("      Deposit:     {} atomic", entry.deposit);
        println!("      Spent:       {} atomic", entry.cumulative_amount);
        println!("      Escrow:      {}", entry.escrow_contract);
        println!("      Chain:       {}", entry.chain_id);
        println!("      Signer:      {}", entry.authorized_signer);
        if entry.close_requested_at > 0 {
            let withdraw_after = entry.close_requested_at + GRACE_PERIOD_SECS;
            if now >= withdraw_after {
                println!("      Status:      READY TO WITHDRAW");
            } else {
                let remaining = withdraw_after - now;
                println!(
                    "      Status:      Closing (withdraw in {}m {}s)",
                    remaining / 60,
                    remaining % 60
                );
            }
        } else {
            println!("      Status:      Open");
        }
        println!();
    }

    Ok(())
}

fn resolve_channel_keys(
    state: &StreamState,
    channel: &Option<String>,
    all: bool,
) -> Result<Vec<String>> {
    if all {
        return Ok(state.channels.keys().cloned().collect());
    }
    if let Some(ref selector) = channel {
        if let Ok(idx) = selector.parse::<usize>() {
            let key = state
                .channels
                .keys()
                .nth(idx)
                .cloned()
                .context(format!("No channel at index {idx}"))?;
            return Ok(vec![key]);
        }
        let key = state
            .channels
            .keys()
            .find(|k| k.contains(selector))
            .cloned()
            .context(format!("No channel matching '{selector}'"))?;
        return Ok(vec![key]);
    }
    if state.channels.len() == 1 {
        return Ok(state.channels.keys().cloned().collect());
    }
    bail!(
        "Multiple channels open ({}). Specify an index or use --all.",
        state.channels.len()
    );
}

pub async fn close_channel(
    channel: Option<String>,
    all: bool,
    config_path: Option<&str>,
) -> Result<()> {
    let mut state = StreamState::load()?;

    if state.channels.is_empty() {
        println!("No open stream channels.");
        return Ok(());
    }

    let keys = resolve_channel_keys(&state, &channel, all)?;
    let config = load_config(config_path.map(String::from).as_ref())?;
    let signer_ctx = load_signer_with_priority()?;
    let signer = signer_ctx.signer;
    let mut closed = 0usize;

    for key in &keys {
        let entry = state.channels.get(key).unwrap().clone();

        let channel_id: B256 = entry
            .channel_id
            .parse()
            .context("Invalid stored channel ID")?;
        let escrow_contract: Address = entry
            .escrow_contract
            .parse()
            .context("Invalid stored escrow contract")?;
        let token: Address = entry.token.parse().context("Invalid stored token")?;

        let network = Network::from_chain_id(entry.chain_id)
            .context(format!("Unknown chain ID {}", entry.chain_id))?;
        let network_info = config.resolve_network(network.as_str())?;
        let chain_id = entry.chain_id;
        let gas_config = network.gas_config();

        let rpc_url: reqwest::Url = network_info.rpc_url.parse().context("Invalid RPC URL")?;
        let provider = ProviderBuilder::new().connect_http(rpc_url);

        let on_chain = query_on_chain_channel(&provider, escrow_contract, channel_id).await?;
        if let Some(ref ch) = on_chain {
            if ch.finalized {
                println!("Channel {} already finalized on-chain.", entry.channel_id);
                continue;
            }
            if ch.close_requested_at > 0 {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let withdraw_after = ch.close_requested_at + GRACE_PERIOD_SECS;
                if now < withdraw_after {
                    let remaining = withdraw_after - now;
                    println!(
                        "Channel {} already has close requested. Withdraw available in {}m {}s.",
                        entry.channel_id,
                        remaining / 60,
                        remaining % 60
                    );
                } else {
                    println!(
                        "Channel {} close grace period elapsed. Run `tempoctl stream withdraw`.",
                        entry.channel_id
                    );
                }
                if entry.close_requested_at == 0 {
                    let e = state.channels.get_mut(key).unwrap();
                    e.close_requested_at = ch.close_requested_at;
                    closed += 1;
                }
                continue;
            }
        }

        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .context("Invalid wallet address")?;
        let from = wallet_address.unwrap_or_else(|| signer.address());

        let nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .context("Failed to get nonce")?;

        let calldata = encode_escrow_request_close(channel_id.0);
        let calls = vec![Call {
            to: alloy::primitives::TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: calldata,
        }];

        let signed_tx = create_tempo_transaction_with_calls(
            &signer,
            chain_id,
            nonce,
            token,
            calls,
            &gas_config,
            gas_config.gas_limit,
            wallet_address,
            None,
        )?;

        let tx_bytes = hex::decode(&signed_tx).context("Invalid signed tx hex")?;
        let pending = provider
            .send_raw_transaction(&tx_bytes)
            .await
            .context("Failed to broadcast requestClose transaction")?;

        let tx_hash = *pending.tx_hash();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let e = state.channels.get_mut(key).unwrap();
        e.close_requested_at = now;
        closed += 1;

        info!(
            channel_id = %entry.channel_id,
            tx_hash = %tx_hash,
            "requested channel close on-chain"
        );

        println!("Requested close for channel {}", entry.channel_id);
        println!("  TX: {tx_hash}");
        println!("  Withdraw available after 15-minute grace period.");
        println!("  Run: tempoctl stream withdraw");
    }

    if closed > 0 {
        state.save()?;
    }

    Ok(())
}

pub async fn withdraw_channel(
    channel: Option<String>,
    all: bool,
    config_path: Option<&str>,
) -> Result<()> {
    let mut state = StreamState::load()?;

    if state.channels.is_empty() {
        println!("No open stream channels.");
        return Ok(());
    }

    let keys = resolve_channel_keys(&state, &channel, all)?;
    let config = load_config(config_path.map(String::from).as_ref())?;
    let signer_ctx = load_signer_with_priority()?;
    let signer = signer_ctx.signer;
    let mut withdrawn = 0usize;

    for key in &keys {
        let entry = state.channels.get(key).unwrap().clone();

        let channel_id: B256 = entry
            .channel_id
            .parse()
            .context("Invalid stored channel ID")?;
        let escrow_contract: Address = entry
            .escrow_contract
            .parse()
            .context("Invalid stored escrow contract")?;
        let token: Address = entry.token.parse().context("Invalid stored token")?;

        let network = Network::from_chain_id(entry.chain_id)
            .context(format!("Unknown chain ID {}", entry.chain_id))?;
        let network_info = config.resolve_network(network.as_str())?;
        let chain_id = entry.chain_id;
        let gas_config = network.gas_config();

        let rpc_url: reqwest::Url = network_info.rpc_url.parse().context("Invalid RPC URL")?;
        let provider = ProviderBuilder::new().connect_http(rpc_url);

        let on_chain = query_on_chain_channel(&provider, escrow_contract, channel_id).await?;
        match on_chain {
            Some(ref ch) if ch.finalized => {
                println!("Channel {} already finalized.", entry.channel_id);
                state.channels.remove(key);
                withdrawn += 1;
                continue;
            }
            Some(ref ch) if ch.close_requested_at == 0 => {
                println!(
                    "Channel {} has no close request. Run `tempoctl stream close` first.",
                    entry.channel_id
                );
                continue;
            }
            Some(ref ch) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let withdraw_after = ch.close_requested_at + GRACE_PERIOD_SECS;
                if now < withdraw_after {
                    let remaining = withdraw_after - now;
                    let mins = remaining / 60;
                    let secs = remaining % 60;
                    println!(
                        "Channel {} grace period not elapsed. Withdraw available in {}m {}s.",
                        entry.channel_id, mins, secs
                    );
                    continue;
                }
            }
            None => {
                println!("Channel {} not found on-chain.", entry.channel_id);
                state.channels.remove(key);
                withdrawn += 1;
                continue;
            }
        }

        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .context("Invalid wallet address")?;
        let from = wallet_address.unwrap_or_else(|| signer.address());

        let nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .context("Failed to get nonce")?;

        let calldata = encode_escrow_withdraw(channel_id.0);
        let calls = vec![Call {
            to: alloy::primitives::TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: calldata,
        }];

        let signed_tx = create_tempo_transaction_with_calls(
            &signer,
            chain_id,
            nonce,
            token,
            calls,
            &gas_config,
            gas_config.gas_limit,
            wallet_address,
            None,
        )?;

        let tx_bytes = hex::decode(&signed_tx).context("Invalid signed tx hex")?;
        let pending = provider
            .send_raw_transaction(&tx_bytes)
            .await
            .context("Failed to broadcast withdraw transaction")?;

        let tx_hash = *pending.tx_hash();

        println!("Withdraw TX broadcast: {tx_hash}");
        println!("  Waiting for confirmation...");

        let status = poll_receipt_status(&provider, tx_hash).await?;

        if !status {
            println!(
                "  Withdraw TX reverted for channel {}. Channel not removed from state.",
                entry.channel_id
            );
            continue;
        }

        info!(
            channel_id = %entry.channel_id,
            tx_hash = %tx_hash,
            "withdrew remaining deposit from channel"
        );

        println!("Withdrew from channel {}", entry.channel_id);
        println!("  TX: {tx_hash}");

        state.channels.remove(key);
        withdrawn += 1;
    }

    state.save()?;
    if withdrawn > 0 {
        println!("Withdrawn {} channel(s). State updated.", withdrawn);
    }

    Ok(())
}

async fn poll_receipt_status(
    provider: &impl Provider,
    tx_hash: alloy::primitives::TxHash,
) -> Result<bool> {
    use std::time::Duration;

    for _ in 0..60 {
        let receipt: Option<serde_json::Value> = provider
            .raw_request("eth_getTransactionReceipt".into(), (tx_hash,))
            .await
            .context("Failed to fetch transaction receipt")?;

        if let Some(r) = receipt {
            let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("0x0");
            return Ok(status == "0x1");
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    bail!("Transaction receipt not found after 120s")
}
