//! Library API - high-level client for making payment-enabled HTTP requests

use crate::config::Config;
use crate::error::{PurlError, Result};
use crate::http::{HttpClient, HttpClientBuilder, HttpResponse};
use crate::payment_provider::PROVIDER_REGISTRY;

/// Builder for making payment-enabled HTTP requests.
///
/// This is the main entry point for making HTTP requests with automatic payment handling.
/// Requests that return a 402 Payment Required status will automatically negotiate payment
/// requirements and submit payment before retrying the request.
///
/// # Example
/// ```no_run
/// # use purl::{Client, Config};
/// # async fn example() -> purl::Result<()> {
/// let client = Client::new()?
///     .max_amount("1000000")
///     .verbose();
///
/// let result = client.get("https://api.example.com/data").await?;
/// # Ok(())
/// # }
/// ```
pub struct Client {
    config: Config,
    max_amount: Option<String>,
    allowed_networks: Vec<String>,
    headers: Vec<(String, String)>,
    timeout: Option<u64>,
    follow_redirects: bool,
    user_agent: Option<String>,
    verbose: bool,
    dry_run: bool,
}

impl Client {
    /// Create a new Client by loading configuration from the default location.
    ///
    /// This loads the config from `~/.config/purl/purl.toml`.
    ///
    /// # Errors
    /// Returns an error if the config file cannot be found or parsed.
    pub fn new() -> Result<Self> {
        let config = Config::load()?;
        Ok(Self::with_config(config))
    }

    /// Create a new Client with the provided configuration.
    ///
    /// Use this when you want to provide configuration programmatically
    /// rather than loading it from a file.
    ///
    /// # Example
    /// ```no_run
    /// # use purl::{Client, Config, EvmConfig};
    /// let config = Config {
    ///     evm: Some(EvmConfig {
    ///         keystore: None,
    ///         private_key: Some("your_key_here".to_string()),
    ///     }),
    ///     ..Default::default()
    /// };
    /// let client = Client::with_config(config);
    /// ```
    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            max_amount: None,
            allowed_networks: Vec::new(),
            headers: Vec::new(),
            timeout: None,
            follow_redirects: false,
            user_agent: None,
            verbose: false,
            dry_run: false,
        }
    }

    /// Set the maximum amount (in token base units) willing to pay.
    ///
    /// If a payment request exceeds this amount, the request will fail
    /// with an `AmountExceedsMax` error.
    #[must_use]
    pub fn max_amount(mut self, amount: impl Into<String>) -> Self {
        self.max_amount = Some(amount.into());
        self
    }

    /// Restrict payments to only these networks.
    ///
    /// If specified, only payment requirements for these networks will be considered.
    /// Pass an empty slice to allow all networks.
    #[must_use]
    pub fn allowed_networks(mut self, networks: &[&str]) -> Self {
        self.allowed_networks = networks.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Add a custom HTTP header to all requests.
    ///
    /// Can be called multiple times to add multiple headers.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Set the HTTP request timeout in seconds.
    #[must_use]
    pub fn timeout(mut self, seconds: u64) -> Self {
        self.timeout = Some(seconds);
        self
    }

    /// Enable automatic following of HTTP redirects.
    #[must_use]
    pub fn follow_redirects(mut self) -> Self {
        self.follow_redirects = true;
        self
    }

    /// Set a custom User-Agent header.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Enable verbose output for debugging.
    #[must_use]
    pub fn verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Enable dry-run mode.
    ///
    /// In dry-run mode, payment requirements are negotiated but no actual
    /// payment is made. Returns `PaymentResult::DryRun` with payment details.
    #[must_use]
    pub fn dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// Perform a GET request to the specified URL.
    ///
    /// If the server responds with 402 Payment Required, payment will be
    /// automatically negotiated and submitted before retrying the request.
    pub async fn get(&self, url: &str) -> Result<PaymentResult> {
        self.request("GET", url, None).await
    }

    /// Perform a POST request to the specified URL with optional body data.
    ///
    /// If the server responds with 402 Payment Required, payment will be
    /// automatically negotiated and submitted before retrying the request.
    pub async fn post(&self, url: &str, data: Option<&[u8]>) -> Result<PaymentResult> {
        self.request("POST", url, data).await
    }

    /// Configure a new HttpClient with the common settings
    fn configure_client(&self, additional_headers: &[(String, String)]) -> Result<HttpClient> {
        let mut builder = HttpClientBuilder::new()
            .verbose(self.verbose)
            .follow_redirects(self.follow_redirects)
            .headers(&self.headers)
            .headers(additional_headers);

        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }

        if let Some(ref ua) = self.user_agent {
            builder = builder.user_agent(ua);
        }

        builder.build()
    }

    /// Execute an HTTP request with the configured method and data
    fn execute_request(
        &self,
        client: &mut HttpClient,
        method: &str,
        url: &str,
        data: Option<&[u8]>,
    ) -> Result<HttpResponse> {
        match method {
            "GET" => client.get(url),
            "POST" => client.post(url, data),
            _ => Err(PurlError::UnsupportedHttpMethod(method.to_string())),
        }
    }

    async fn request(&self, method: &str, url: &str, data: Option<&[u8]>) -> Result<PaymentResult> {
        let mut client = self.configure_client(&[])?;
        let response = self.execute_request(&mut client, method, url, data)?;

        if !response.is_payment_required() {
            return Ok(PaymentResult::Success(response));
        }

        if response
            .get_header(crate::protocol::web::WWW_AUTHENTICATE_HEADER)
            .is_some()
        {
            self.handle_web_payment(response, method, url, data).await
        } else {
            Err(PurlError::MissingHeader("WWW-Authenticate".to_string()))
        }
    }

    async fn handle_web_payment(
        &self,
        response: HttpResponse,
        method: &str,
        url: &str,
        data: Option<&[u8]>,
    ) -> Result<PaymentResult> {
        use crate::payment_provider::DryRunInfo;
        use crate::protocol::web::{parse_receipt, parse_www_authenticate, PaymentIntent};

        let www_auth = response
            .get_header(crate::protocol::web::WWW_AUTHENTICATE_HEADER)
            .ok_or_else(|| PurlError::MissingHeader("WWW-Authenticate".to_string()))?;

        let challenge = parse_www_authenticate(www_auth)?;

        if !challenge.method.is_supported() {
            return Err(PurlError::unsupported_method(&challenge.method));
        }
        if challenge.intent != PaymentIntent::Charge {
            return Err(PurlError::UnsupportedPaymentIntent(format!(
                "Only 'charge' intent is supported, got: {:?}",
                challenge.intent
            )));
        }

        let network_name = challenge
            .method
            .network_name()
            .ok_or_else(|| PurlError::unsupported_method(&challenge.method))?;

        let charge_req: crate::protocol::web::ChargeRequest =
            serde_json::from_value(challenge.request.clone()).map_err(|e| {
                PurlError::InvalidChallenge(format!("Invalid charge request: {}", e))
            })?;

        if let Some(ref max_amount) = self.max_amount {
            let amount: u128 = charge_req
                .amount
                .parse()
                .map_err(|e| PurlError::InvalidAmount(format!("Invalid amount: {}", e)))?;
            let max: u128 = max_amount
                .parse()
                .map_err(|e| PurlError::InvalidAmount(format!("Invalid max amount: {}", e)))?;
            if amount > max {
                return Err(PurlError::AmountExceedsMax {
                    required: amount,
                    max,
                });
            }
        }

        if !self.allowed_networks.is_empty()
            && !self.allowed_networks.contains(&network_name.to_string())
        {
            return Err(PurlError::NoCompatibleMethod {
                networks: vec![network_name.to_string()],
            });
        }

        if self.dry_run {
            let provider = PROVIDER_REGISTRY
                .find_provider(network_name)
                .ok_or_else(|| PurlError::ProviderNotFound(network_name.to_string()))?;
            let address = provider.get_address(&self.config)?;
            return Ok(PaymentResult::DryRun(DryRunInfo {
                provider: "EVM".to_string(),
                network: network_name.to_string(),
                amount: charge_req.amount.clone(),
                asset: charge_req.asset.clone(),
                from: address,
                to: charge_req.destination.clone(),
                estimated_fee: None,
            }));
        }

        let provider = PROVIDER_REGISTRY
            .find_provider(network_name)
            .ok_or_else(|| PurlError::ProviderNotFound(network_name.to_string()))?;

        let credential = provider
            .create_web_payment(&challenge, &self.config)
            .await?;

        let auth_header = crate::protocol::web::format_authorization(&credential)?;

        let payment_header = vec![("Authorization".to_string(), auth_header)];
        let mut client = self.configure_client(&payment_header)?;
        let response = self.execute_request(&mut client, method, url, data)?;

        let receipt = if let Some(receipt_header) =
            response.get_header(crate::protocol::web::PAYMENT_RECEIPT_HEADER)
        {
            Some(parse_receipt(receipt_header)?)
        } else {
            None
        };

        Ok(PaymentResult::WebPaid { response, receipt })
    }
}

/// The result of an HTTP request that may have required payment.
#[derive(Debug)]
pub enum PaymentResult {
    Success(HttpResponse),
    WebPaid {
        response: HttpResponse,
        receipt: Option<crate::protocol::web::PaymentReceipt>,
    },

    /// Dry-run mode was enabled, so payment was not actually made.
    ///
    /// Contains information about what payment would have been made,
    /// including amount, asset, sender, recipient, and any warnings.
    DryRun(crate::payment_provider::DryRunInfo),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EvmConfig;

    /// Test EVM private key (DO NOT use in production)
    const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

    fn test_config() -> Config {
        Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_client_with_config() {
        let config = test_config();
        let client = Client::with_config(config);

        assert!(client.max_amount.is_none());
        assert!(client.allowed_networks.is_empty());
        assert!(client.headers.is_empty());
        assert!(client.timeout.is_none());
        assert!(!client.follow_redirects);
        assert!(client.user_agent.is_none());
        assert!(!client.verbose);
        assert!(!client.dry_run);
    }

    #[test]
    fn test_client_max_amount() {
        let config = test_config();
        let client = Client::with_config(config).max_amount("1000000");

        assert_eq!(client.max_amount, Some("1000000".to_string()));
    }

    #[test]
    fn test_client_max_amount_from_various_types() {
        let config = test_config();

        let client = Client::with_config(config.clone()).max_amount("1000000");
        assert_eq!(client.max_amount, Some("1000000".to_string()));

        let client = Client::with_config(config.clone()).max_amount(String::from("2000000"));
        assert_eq!(client.max_amount, Some("2000000".to_string()));
    }

    #[test]
    fn test_client_allowed_networks() {
        let config = test_config();
        let client = Client::with_config(config).allowed_networks(&["base", "ethereum"]);

        assert_eq!(client.allowed_networks.len(), 2);
        assert!(client.allowed_networks.contains(&"base".to_string()));
        assert!(client.allowed_networks.contains(&"ethereum".to_string()));
    }

    #[test]
    fn test_client_allowed_networks_empty() {
        let config = test_config();
        let client = Client::with_config(config).allowed_networks(&[]);

        assert!(client.allowed_networks.is_empty());
    }

    #[test]
    fn test_client_header() {
        let config = test_config();
        let client = Client::with_config(config)
            .header("X-Custom-Header", "value1")
            .header("X-Another-Header", "value2");

        assert_eq!(client.headers.len(), 2);
        assert!(client
            .headers
            .contains(&("X-Custom-Header".to_string(), "value1".to_string())));
        assert!(client
            .headers
            .contains(&("X-Another-Header".to_string(), "value2".to_string())));
    }

    #[test]
    fn test_client_timeout() {
        let config = test_config();
        let client = Client::with_config(config).timeout(30);

        assert_eq!(client.timeout, Some(30));
    }

    #[test]
    fn test_client_follow_redirects() {
        let config = test_config();
        let client = Client::with_config(config).follow_redirects();

        assert!(client.follow_redirects);
    }

    #[test]
    fn test_client_user_agent() {
        let config = test_config();
        let client = Client::with_config(config).user_agent("MyApp/1.0");

        assert_eq!(client.user_agent, Some("MyApp/1.0".to_string()));
    }

    #[test]
    fn test_client_verbose() {
        let config = test_config();
        let client = Client::with_config(config).verbose();

        assert!(client.verbose);
    }

    #[test]
    fn test_client_dry_run() {
        let config = test_config();
        let client = Client::with_config(config).dry_run();

        assert!(client.dry_run);
    }

    #[test]
    fn test_client_builder_chaining() {
        let config = test_config();
        let client = Client::with_config(config)
            .max_amount("1000000")
            .allowed_networks(&["base"])
            .header("Authorization", "Bearer token")
            .timeout(60)
            .follow_redirects()
            .user_agent("TestAgent/1.0")
            .verbose()
            .dry_run();

        assert_eq!(client.max_amount, Some("1000000".to_string()));
        assert_eq!(client.allowed_networks, vec!["base".to_string()]);
        assert_eq!(client.headers.len(), 1);
        assert_eq!(client.timeout, Some(60));
        assert!(client.follow_redirects);
        assert_eq!(client.user_agent, Some("TestAgent/1.0".to_string()));
        assert!(client.verbose);
        assert!(client.dry_run);
    }

    #[test]
    fn test_payment_result_variants() {
        use std::collections::HashMap;

        let _success = PaymentResult::Success(HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: b"success".to_vec(),
        });

        let _web_paid = PaymentResult::WebPaid {
            response: HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: b"web_paid".to_vec(),
            },
            receipt: None,
        };

        let _dry_run = PaymentResult::DryRun(crate::payment_provider::DryRunInfo {
            provider: "EVM".to_string(),
            network: "base".to_string(),
            amount: "1000000".to_string(),
            asset: "USDC".to_string(),
            from: "0x123".to_string(),
            to: "0x456".to_string(),
            estimated_fee: Some("0".to_string()),
        });
    }

    #[test]
    fn test_configure_client() {
        let config = test_config();
        let client = Client::with_config(config)
            .timeout(30)
            .follow_redirects()
            .user_agent("TestAgent/1.0")
            .header("X-Custom", "value");

        let result = client.configure_client(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_configure_client_with_additional_headers() {
        let config = test_config();
        let client = Client::with_config(config).header("X-Existing", "existing");

        let additional = vec![("X-Additional".to_string(), "additional".to_string())];
        let result = client.configure_client(&additional);
        assert!(result.is_ok());
    }
}
