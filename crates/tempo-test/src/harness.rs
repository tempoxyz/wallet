//! Pre-built test harnesses that compose mock servers with configuration.

use tempfile::TempDir;

use crate::config::TestConfigBuilder;
use crate::mock_http::{charge_www_authenticate, MockServer};
use crate::mock_rpc::MockRpcServer;
use crate::wallet::{MODERATO_DIRECT_KEYS_TOML, MODERATO_KEYCHAIN_KEYS_TOML};

/// Complete harness for 402→payment→200 integration tests.
///
/// Bundles a mock RPC server, a mock HTTP payment server, and a temp
/// directory with wallet config — all wired together.
pub struct PaymentTestHarness {
    /// Mock RPC server (keep alive for the duration of the test).
    pub rpc: MockRpcServer,
    /// Mock HTTP server (402→200 flow).
    pub server: MockServer,
    /// Temp directory with config.toml + keys.toml.
    pub temp: TempDir,
}

impl PaymentTestHarness {
    /// Standard Moderato charge flow with Direct signing mode.
    pub async fn charge() -> Self {
        Self::charge_with_body("ok").await
    }

    /// Charge flow with a custom success body.
    pub async fn charge_with_body(body: &str) -> Self {
        Self::build(body, MODERATO_DIRECT_KEYS_TOML, "test-charge").await
    }

    /// Charge flow with a custom challenge ID and success body.
    pub async fn charge_with_id(id: &str, body: &str) -> Self {
        Self::build(body, MODERATO_DIRECT_KEYS_TOML, id).await
    }

    /// Charge flow with Keychain signing mode.
    pub async fn charge_keychain(body: &str) -> Self {
        Self::build(body, MODERATO_KEYCHAIN_KEYS_TOML, "test-kc").await
    }

    /// Charge flow that also returns a Payment-Receipt header.
    pub async fn charge_with_receipt(body: &str, receipt: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate("test-receipt");
        let server = MockServer::start_payment_with_receipt(&www_auth, body, receipt).await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    async fn build(body: &str, keys_toml: &str, id: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate(id);
        let server = MockServer::start_payment(&www_auth, body).await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(keys_toml)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    /// Get the full URL for a path on the mock HTTP server.
    pub fn url(&self, path: &str) -> String {
        self.server.url(path)
    }
}
