//! Network command handlers for listing and inspecting supported networks.

use crate::cli::OutputFormat;
use anyhow::{Context, Result};
use purl::network::{ChainType, Network};
use serde::Serialize;

/// Display data for a network in list output
#[derive(Debug, Serialize)]
struct NetworkListItem {
    name: String,
    display_name: String,
    #[serde(rename = "type")]
    chain_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    chain_id: Option<u64>,
    network_type: String,
}

/// Detailed information about a network
#[derive(Debug, Serialize)]
struct NetworkDetail {
    name: String,
    display_name: String,
    #[serde(rename = "type")]
    chain_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    chain_id: Option<u64>,
    mainnet: bool,
    testnet: bool,
}

impl From<Network> for NetworkListItem {
    fn from(network: Network) -> Self {
        let info = network.info();
        NetworkListItem {
            name: network.as_str().to_string(),
            display_name: info.display_name.to_string(),
            chain_type: match info.chain_type {
                ChainType::Evm => "EVM".to_string(),
            },
            chain_id: info.chain_id,
            network_type: if info.mainnet {
                "mainnet".to_string()
            } else {
                "testnet".to_string()
            },
        }
    }
}

impl From<Network> for NetworkDetail {
    fn from(network: Network) -> Self {
        let info = network.info();
        NetworkDetail {
            name: network.as_str().to_string(),
            display_name: info.display_name.to_string(),
            chain_type: match info.chain_type {
                ChainType::Evm => "EVM".to_string(),
            },
            chain_id: info.chain_id,
            mainnet: info.mainnet,
            testnet: info.is_testnet(),
        }
    }
}

/// List all supported networks
pub fn list_networks(output_format: OutputFormat) -> Result<()> {
    let networks: Vec<NetworkListItem> = Network::all()
        .iter()
        .map(|net| NetworkListItem::from(*net))
        .collect();

    match output_format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&networks).context("Failed to serialize to JSON")?;
            println!("{json}");
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&networks).context("Failed to serialize to YAML")?;
            println!("{yaml}");
        }
        OutputFormat::Text => {
            println!(
                "{:<20} {:<25} {:<10} {:<12} {:<10}",
                "NAME", "DISPLAY NAME", "TYPE", "CHAIN ID", "NETWORK"
            );
            println!("{}", "-".repeat(80));

            for network in networks {
                let chain_id_str = network
                    .chain_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "N/A".to_string());

                println!(
                    "{:<20} {:<25} {:<10} {:<12} {:<10}",
                    network.name,
                    network.display_name,
                    network.chain_type,
                    chain_id_str,
                    network.network_type
                );
            }
        }
    }

    Ok(())
}

/// Show detailed information about a specific network
pub fn show_network_info(network_name: &str, output_format: OutputFormat) -> Result<()> {
    let network: Network = network_name.parse().map_err(|_| {
        anyhow::anyhow!(
            "Unknown network: '{network_name}'. Use 'purl networks list' to see available networks."
        )
    })?;

    let detail = NetworkDetail::from(network);

    match output_format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&detail).context("Failed to serialize to JSON")?;
            println!("{json}");
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&detail).context("Failed to serialize to YAML")?;
            println!("{yaml}");
        }
        OutputFormat::Text => {
            println!("Network Information");
            println!("{}", "=".repeat(40));
            println!("Name:         {}", detail.name);
            println!("Display Name: {}", detail.display_name);
            println!("Type:         {}", detail.chain_type);

            if let Some(chain_id) = detail.chain_id {
                println!("Chain ID:     {chain_id}");
            }

            println!(
                "Mainnet:      {}",
                if detail.mainnet { "yes" } else { "no" }
            );
            println!(
                "Testnet:      {}",
                if detail.testnet { "yes" } else { "no" }
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_networks_text() {
        // Should not panic
        let result = list_networks(OutputFormat::Text);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_networks_json() {
        let result = list_networks(OutputFormat::Json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_networks_yaml() {
        let result = list_networks(OutputFormat::Yaml);
        assert!(result.is_ok());
    }

    #[test]
    fn test_show_network_info_base() {
        let result = show_network_info("base", OutputFormat::Text);
        assert!(result.is_ok());
    }

    #[test]
    fn test_show_network_info_tempo() {
        let result = show_network_info("tempo-moderato", OutputFormat::Text);
        assert!(result.is_ok());
    }

    #[test]
    fn test_show_network_info_unknown() {
        let result = show_network_info("unknown-network", OutputFormat::Text);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown network"));
    }

    #[test]
    fn test_network_list_item_conversion() {
        let network = Network::Base;
        let item = NetworkListItem::from(network);

        assert_eq!(item.name, "base");
        assert_eq!(item.display_name, "Base");
        assert_eq!(item.chain_type, "EVM");
        assert_eq!(item.chain_id, Some(8453));
        assert_eq!(item.network_type, "mainnet");
    }

    #[test]
    fn test_network_detail_conversion() {
        let network = Network::BaseSepolia;
        let detail = NetworkDetail::from(network);

        assert_eq!(detail.name, "base-sepolia");
        assert_eq!(detail.display_name, "Base Sepolia");
        assert_eq!(detail.chain_type, "EVM");
        assert_eq!(detail.chain_id, Some(84532));
        assert!(!detail.mainnet);
        assert!(detail.testnet);
    }
}
