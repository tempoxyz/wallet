//! Directory fetching, parsing, and caching.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Result, TempoCtlError};

const DIRECTORY_URL: &str = "https://payments.tempo.xyz/directory";
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const CACHE_FILE: &str = "directory.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDirectory {
    fetched_at: u64,
    directory: Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directory {
    pub version: String,
    pub timestamp: String,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub url: String,
    pub pricing: Pricing,
    #[serde(default)]
    pub streaming: StreamingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    pub default: String,
    pub asset: String,
    pub destination: String,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub path: String,
    pub methods: Vec<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default, rename = "requiresPayment")]
    pub requires_payment: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamingConfig {
    #[serde(default)]
    pub supported: bool,
    #[serde(default, rename = "escrowContract")]
    pub escrow_contract: Option<String>,
    #[serde(default, rename = "defaultDeposit")]
    pub default_deposit: Option<String>,
}

impl Directory {
    fn cache_path() -> Result<PathBuf> {
        Ok(Config::default_config_dir()?.join(CACHE_FILE))
    }

    fn load_from_cache() -> Result<Option<Directory>> {
        let path = Self::cache_path()?;

        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path)?;
        let cached: CachedDirectory = serde_json::from_str(&contents)?;

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if now - cached.fetched_at > CACHE_TTL.as_secs() {
            return Ok(None);
        }

        Ok(Some(cached.directory))
    }

    fn save_to_cache(&self) -> Result<()> {
        let path = Self::cache_path()?;

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cached = CachedDirectory {
            fetched_at: now,
            directory: self.clone(),
        };

        let contents = serde_json::to_string_pretty(&cached)?;
        fs::write(&path, contents)?;

        Ok(())
    }

    async fn fetch() -> Result<Directory> {
        let client = reqwest::Client::new();
        let response = client
            .get(DIRECTORY_URL)
            .send()
            .await
            .map_err(|e| TempoCtlError::Http(format!("Failed to fetch directory: {}", e)))?;

        if !response.status().is_success() {
            return Err(TempoCtlError::Http(format!(
                "Directory fetch failed: {}",
                response.status()
            )));
        }

        let directory: Directory = response
            .json()
            .await
            .map_err(|e| TempoCtlError::Http(format!("Failed to parse directory: {}", e)))?;

        Ok(directory)
    }

    pub async fn load(refresh: bool) -> Result<Directory> {
        if !refresh {
            if let Some(cached) = Self::load_from_cache()? {
                return Ok(cached);
            }
        }

        let directory = Self::fetch().await?;
        let _ = directory.save_to_cache();
        Ok(directory)
    }

    pub fn find_service(&self, name: &str) -> Option<&Service> {
        let name_lower = name.to_lowercase();

        self.services.iter().find(|s| {
            s.slug.to_lowercase() == name_lower
                || s.aliases.iter().any(|a| a.to_lowercase() == name_lower)
        })
    }
}

impl Service {
    #[allow(dead_code)]
    pub fn build_url(&self, path: &str) -> String {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };

        format!("{}{}", self.url, path)
    }

    pub fn aliases_string(&self) -> String {
        if self.aliases.is_empty() {
            String::new()
        } else {
            self.aliases.join(", ")
        }
    }
}

impl Endpoint {
    pub fn price_display(&self) -> String {
        match &self.price {
            Some(price_str) => {
                if let Ok(price) = price_str.parse::<u128>() {
                    format_price(price)
                } else {
                    "varies".to_string()
                }
            }
            None => "varies".to_string(),
        }
    }
}

pub fn format_price(micros: u128) -> String {
    let dollars = micros as f64 / 1_000_000.0;
    if dollars < 0.01 {
        format!("${:.4}", dollars)
    } else {
        format!("${:.2}", dollars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_price_cents() {
        assert_eq!(format_price(10_000), "$0.01");
        assert_eq!(format_price(50_000), "$0.05");
    }

    #[test]
    fn test_format_price_dollars() {
        assert_eq!(format_price(1_000_000), "$1.00");
    }

    #[test]
    fn test_format_price_small() {
        assert_eq!(format_price(1_000), "$0.0010");
    }

    #[test]
    fn test_service_build_url() {
        let service = Service {
            name: "Test".to_string(),
            slug: "test".to_string(),
            aliases: vec![],
            url: "https://test.payments.tempo.xyz".to_string(),
            pricing: Pricing {
                default: "10000".to_string(),
                asset: "0x123".to_string(),
                destination: "0x456".to_string(),
                endpoints: vec![],
            },
            streaming: StreamingConfig::default(),
        };

        assert_eq!(
            service.build_url("/v1/test"),
            "https://test.payments.tempo.xyz/v1/test"
        );
        assert_eq!(
            service.build_url("v1/test"),
            "https://test.payments.tempo.xyz/v1/test"
        );
    }

    #[test]
    fn test_find_service() {
        let directory = Directory {
            version: "1.0.0".to_string(),
            timestamp: "2024-01-01".to_string(),
            services: vec![Service {
                name: "OpenAI".to_string(),
                slug: "openai".to_string(),
                aliases: vec!["gpt".to_string()],
                url: "https://openai.payments.tempo.xyz".to_string(),
                pricing: Pricing {
                    default: "10000".to_string(),
                    asset: "0x123".to_string(),
                    destination: "0x456".to_string(),
                    endpoints: vec![],
                },
                streaming: StreamingConfig::default(),
            }],
        };

        assert!(directory.find_service("openai").is_some());
        assert!(directory.find_service("OpenAI").is_some());
        assert!(directory.find_service("gpt").is_some());
        assert!(directory.find_service("unknown").is_none());
    }
}
