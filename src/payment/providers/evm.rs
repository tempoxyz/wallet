//! EVM payment provider implementation.
//!
//! This module provides EVM-specific payment functionality with support for:
//! - EIP-1559 transactions
//! - ERC20 token transfers
//! - Memo support via transferWithMemo

use crate::config::Config;
use crate::error::{PgetError, Result, ResultExt, SigningContext};
use crate::network::{GasConfig, Network};
use crate::payment::currency::Currency;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::mpay_ext::{ChargeRequestExt, TempoChargeExt};
use crate::payment::provider::NetworkBalance;
use crate::wallet::signer::WalletSource;
use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use std::str::FromStr;

/// Create an EVM payment credential for a Web Payment Auth challenge.
pub async fn create_evm_payment(
    config: &Config,
    challenge: &mpay::PaymentChallenge,
) -> Result<mpay::PaymentCredential> {
    use crate::payment::mpay_ext::method_to_network;
    use alloy::primitives::Bytes;
    use alloy::providers::Provider;
    use alloy::sol_types::SolCall;
    use mpay::{ChargeRequest, PaymentCredential, PaymentPayload};

    let charge_req: ChargeRequest = challenge
        .request
        .decode()
        .map_err(|e| PgetError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

    let evm_config = config.require_evm()?;
    let signer = evm_config.load_signer(None)?;
    let from = signer.address();

    let currency = charge_req.currency_address()?;
    let recipient = charge_req.recipient_address()?;
    let amount = charge_req.amount_u256()?;
    let memo = charge_req.memo();

    let transfer_data = if let Some(memo_bytes) = memo {
        sol! {
            function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
        }
        let call = transferWithMemoCall {
            to: recipient,
            amount,
            memo: memo_bytes.into(),
        };
        Bytes::from(call.abi_encode())
    } else {
        sol! {
            function transfer(address to, uint256 amount) external returns (bool);
        }
        let call = transferCall {
            to: recipient,
            amount,
        };
        Bytes::from(call.abi_encode())
    };

    let network_name = method_to_network(&challenge.method).ok_or_else(|| {
        PgetError::UnsupportedPaymentMethod(format!(
            "Unsupported payment method: {}",
            challenge.method
        ))
    })?;

    let network_info = config.resolve_network(network_name)?;
    let chain_id = network_info.chain_id.ok_or_else(|| {
        PgetError::InvalidConfig(format!("{} network missing chain ID", network_name))
    })?;

    let gas_config = Network::from_str(network_name)
        .ok()
        .and_then(|n| n.gas_config())
        .unwrap_or(GasConfig::DEFAULT);

    let provider = ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(
        |e| PgetError::InvalidConfig(format!("Invalid RPC URL for {}: {}", network_name, e)),
    )?);

    let nonce = provider
        .get_transaction_count(from)
        .pending()
        .await
        .with_signing_context(SigningContext {
            network: Some(network_name.to_string()),
            address: Some(format!("{:#x}", from)),
            operation: "get_nonce",
        })?;

    let signed_tx =
        create_eip1559_transaction(&signer, chain_id, nonce, currency, transfer_data, &gas_config)
            .await?;

    let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

    Ok(PaymentCredential {
        challenge: challenge.to_echo(),
        source: Some(did),
        payload: PaymentPayload::transaction(format!("0x{}", signed_tx)),
    })
}

/// Query ERC20 token balance for an address on a network.
pub async fn query_erc20_balance(
    config: &Config,
    address: &str,
    network: Network,
    currency: Currency,
) -> Result<NetworkBalance> {
    sol! {
        #[sol(rpc)]
        interface IERC20 {
            function balanceOf(address account) external view returns (uint256);
        }
    }

    let token_config = network.usdc_config().ok_or_else(|| {
        PgetError::UnsupportedToken(format!(
            "Network {} does not support {}",
            network, currency.symbol
        ))
    })?;

    let network_info = config.resolve_network(network.as_str())?;
    let provider = ProviderBuilder::new().connect_http(
        network_info
            .rpc_url
            .parse()
            .map_err(|e| PgetError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}")))?,
    );

    let user_addr = Address::from_str(address)
        .map_err(|e| PgetError::invalid_address(format!("Invalid Ethereum address: {e}")))?;
    let token_addr = Address::from_str(token_config.address).map_err(|e| {
        PgetError::invalid_address(format!(
            "Invalid {} contract address for {}: {}",
            token_config.currency.symbol, network, e
        ))
    })?;

    let contract = IERC20::new(token_addr, &provider);

    let balance = contract.balanceOf(user_addr).call().await.map_err(|e| {
        PgetError::BalanceQuery(format!(
            "Failed to get {} balance for {} on {}: {}",
            token_config.currency.symbol, address, network, e
        ))
    })?;

    let balance_human = format_u256_with_decimals(balance, token_config.currency.decimals);

    Ok(NetworkBalance::new(
        network,
        balance,
        balance_human,
        token_config.currency.symbol.to_string(),
    ))
}

/// Create a standard EIP-1559 transaction with network-specific gas configuration.
async fn create_eip1559_transaction(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    asset: Address,
    transfer_data: alloy::primitives::Bytes,
    gas_config: &GasConfig,
) -> Result<String> {
    use alloy::consensus::Signed;
    use alloy::network::TxSigner;
    use alloy::primitives::U256;

    let tx = alloy::consensus::TxEip1559 {
        chain_id,
        nonce,
        gas_limit: gas_config.gas_limit,
        max_fee_per_gas: gas_config.max_fee_per_gas_u128(),
        max_priority_fee_per_gas: gas_config.max_priority_fee_per_gas_u128(),
        to: alloy::primitives::TxKind::Call(asset),
        value: U256::ZERO,
        access_list: Default::default(),
        input: transfer_data,
    };

    let mut tx_mut = tx.clone();
    let signature = signer
        .sign_transaction(&mut tx_mut)
        .await
        .with_signing_context(SigningContext {
            network: Some(format!("chain_id:{}", chain_id)),
            address: None,
            operation: "sign_eip1559_transaction",
        })?;

    let mut buf = Vec::new();
    buf.push(0x02);
    let signed = Signed::new_unchecked(tx, signature, Default::default());
    signed.rlp_encode(&mut buf);

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_config_defaults() {
        let gas_config = GasConfig::DEFAULT;
        assert_eq!(gas_config.gas_limit, 100_000);
        assert_eq!(gas_config.max_priority_fee_per_gas, 1_000_000_000);
        assert_eq!(gas_config.max_fee_per_gas, 10_000_000_000);

        assert_eq!(
            gas_config.max_fee_per_gas_u128(),
            gas_config.max_fee_per_gas as u128
        );
        assert_eq!(
            gas_config.max_priority_fee_per_gas_u128(),
            gas_config.max_priority_fee_per_gas as u128
        );
    }

    #[test]
    fn test_network_gas_config() {
        assert!(Network::Base.gas_config().is_some());
        assert!(Network::TempoModerato.gas_config().is_some());
        assert!(Network::Ethereum.gas_config().is_some());
    }

    #[test]
    fn test_transfer_encoding() {
        use alloy::primitives::{Address, Bytes, U256};
        use alloy::sol_types::SolCall;

        sol! {
            function transfer(address to, uint256 amount) external returns (bool);
        }

        let recipient: Address = "0x496bc2392ba3b6179a15435ed09dad18d85a1705"
            .parse()
            .unwrap();
        let amount = U256::from(1000u64);

        let call = transferCall {
            to: recipient,
            amount,
        };
        let data = Bytes::from(call.abi_encode());

        assert_eq!(&data[0..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_transfer_with_memo_encoding() {
        use alloy::primitives::{Address, Bytes, U256};
        use alloy::sol_types::SolCall;

        sol! {
            function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
        }

        let recipient: Address = "0x496bc2392ba3b6179a15435ed09dad18d85a1705"
            .parse()
            .unwrap();
        let amount = U256::from(1000u64);
        let memo: [u8; 32] = [
            0xc7, 0x08, 0x64, 0x12, 0x82, 0x16, 0x76, 0xd5, 0x48, 0xdd, 0xcf, 0x3c, 0xc9, 0xb9,
            0xfc, 0x1e, 0xdb, 0x49, 0xc4, 0x53, 0xe6, 0x15, 0xe9, 0x04, 0xf2, 0x84, 0x7b, 0xa7,
            0x9d, 0xd0, 0xec, 0x71,
        ];

        let call = transferWithMemoCall {
            to: recipient,
            amount,
            memo: memo.into(),
        };
        let data = Bytes::from(call.abi_encode());

        assert_eq!(&data[0..4], &[0x95, 0x77, 0x7d, 0x59]);
    }
}
