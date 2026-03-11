//! Shared test infrastructure for Tempo CLI integration tests.
//!
//! Provides mock servers, test configuration builders, assertion helpers,
//! and pre-built test fixtures so that each binary crate's integration
//! tests only need a thin `tests/common/mod.rs` re-export.

pub mod assert;
pub mod command;
pub mod config;
pub mod harness;
pub mod mock_http;
pub mod mock_rpc;
pub mod mock_services;
pub mod session;
pub mod wallet;

pub use assert::*;
pub use command::*;
pub use config::*;
pub use harness::*;
pub use mock_http::*;
pub use mock_rpc::*;
pub use mock_services::*;
pub use session::*;
pub use wallet::*;
