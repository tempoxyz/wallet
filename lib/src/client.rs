//! Library API - high-level client for making x402 payment-enabled requests

use crate::config::Config;
use crate::error::{PurlError, Result};
use crate::http::{HttpClient, HttpClientBuilder, HttpResponse};
use crate::negotiator::PaymentNegotiator;
use crate::payment_provider::PROVIDER_REGISTRY;
use crate::protocol::x402::SettlementResponse;
use base64::Engine;

/// Payment protocol detected from HTTP response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    /// x402 protocol (v1 or v2)
    X402,
    /// Web Payment Auth protocol (IETF draft)
    WebPayment,
}

/// Detect which payment protocol is being used based on HTTP response headers
fn detect_protocol(response: &HttpResponse) -> Protocol {
    // Check for Web Payment Auth (most specific header)
    if let Some(www_auth) = response.get_header(crate::protocol::web::WWW_AUTHENTICATE_HEADER) {
        if www_auth.starts_with(crate::protocol::web::PAYMENT_SCHEME) {
            return Protocol::WebPayment;
        }
    }

    // Default to x402 protocol
    Protocol::X402
}

/// Builder for making x402-enabled HTTP requests.
///
/// This is the main entry point for making HTTP requests with automatic x402 payment handling.
/// Requests that return a 402 Payment Required status will automatically negotiate payment
/// requirements and submit payment before retrying the request.
///
/// # Example
/// ```no_run
/// # use purl_lib::{PurlClient, Config};
/// # async fn example() -> purl_lib::Result<()> {
/// let client = PurlClient::new()?
///     .max_amount("1000000")
///     .verbose();
///
/// let result = client.get("https://api.example.com/data").await?;
/// # Ok(())
/// # }
/// ```
pub struct PurlClient {
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

impl PurlClient {
    /// Create a new PurlClient by loading configuration from the default location.
    ///
    /// This loads the config from `~/.config/purl/purl.toml`.
    ///
    /// # Errors
    /// Returns an error if the config file cannot be found or parsed.
    pub fn new() -> Result<Self> {
        let config = Config::load()?;
        Ok(Self::with_config(config))
    }

    /// Create a new PurlClient with the provided configuration.
    ///
    /// Use this when you want to provide configuration programmatically
    /// rather than loading it from a file.
    ///
    /// # Example
    /// ```no_run
    /// # use purl_lib::{PurlClient, Config, EvmConfig};
    /// let config = Config {
    ///     evm: Some(EvmConfig {
    ///         keystore: None,
    ///         private_key: Some("your_key_here".to_string()),
    ///     }),
    ///     solana: None,
    ///     ..Default::default()
    /// };
    /// let client = PurlClient::with_config(config);
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

        let protocol = detect_protocol(&response);

        match protocol {
            Protocol::X402 => self.handle_x402_payment(response, method, url, data).await,
            Protocol::WebPayment => self.handle_web_payment(response, method, url, data).await,
        }
    }

    async fn handle_x402_payment(
        &self,
        response: HttpResponse,
        method: &str,
        url: &str,
        data: Option<&[u8]>,
    ) -> Result<PaymentResult> {
        let json = response.payment_requirements_json()?;
        let negotiator = PaymentNegotiator::new(&self.config)
            .with_allowed_networks(&self.allowed_networks)
            .with_max_amount(self.max_amount.as_deref());

        let selected = negotiator.select_requirement(&json)?;

        if self.dry_run {
            if let Some(provider) = PROVIDER_REGISTRY.find_provider(selected.network()) {
                let dry_run_info = provider.dry_run(&selected, &self.config)?;
                return Ok(PaymentResult::DryRun(dry_run_info));
            }
        }

        let provider = PROVIDER_REGISTRY
            .find_provider(selected.network())
            .ok_or_else(|| PurlError::ProviderNotFound(selected.network().to_string()))?;

        let payment_payload = provider.create_payment(&selected, &self.config).await?;

        let payload_json = serde_json::to_string(&payment_payload)?;
        let encoded_payload = base64::engine::general_purpose::STANDARD.encode(payload_json);

        // Use version-appropriate header name
        let header_name = payment_payload.payment_header_name();
        let payment_header = vec![(header_name.to_string(), encoded_payload)];
        let mut client = self.configure_client(&payment_header)?;
        let response = self.execute_request(&mut client, method, url, data)?;

        // Check both v1 and v2 response headers
        let response_header_name = payment_payload.response_header_name();
        let settlement = if let Some(header) = response.get_header(response_header_name) {
            let decoded = base64::engine::general_purpose::STANDARD.decode(header)?;
            let settlement: SettlementResponse = serde_json::from_slice(&decoded)?;
            Some(settlement)
        } else {
            None
        };

        Ok(PaymentResult::Paid {
            response,
            settlement,
        })
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

        // Parse WWW-Authenticate header
        let www_auth = response
            .get_header(crate::protocol::web::WWW_AUTHENTICATE_HEADER)
            .ok_or_else(|| PurlError::MissingHeader("WWW-Authenticate".to_string()))?;

        let challenge = parse_www_authenticate(www_auth)?;

        // Validate method and intent
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

        // Check network filter if provided
        if !self.allowed_networks.is_empty()
            && !self.allowed_networks.contains(&network_name.to_string())
        {
            return Err(PurlError::NoCompatibleMethod {
                networks: vec![network_name.to_string()],
            });
        }

        // Dry run check
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

        // Create payment credential
        let provider = PROVIDER_REGISTRY
            .find_provider(network_name)
            .ok_or_else(|| PurlError::ProviderNotFound(network_name.to_string()))?;

        let credential = provider
            .create_web_payment(&challenge, &self.config)
            .await?;

        // Format Authorization header
        let auth_header = crate::protocol::web::format_authorization(&credential)?;

        // Retry request with Authorization header
        let payment_header = vec![("Authorization".to_string(), auth_header)];
        let mut client = self.configure_client(&payment_header)?;
        let response = self.execute_request(&mut client, method, url, data)?;

        // Parse Payment-Receipt header if present
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
    Paid {
        response: HttpResponse,
        settlement: Option<SettlementResponse>,
    },
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
