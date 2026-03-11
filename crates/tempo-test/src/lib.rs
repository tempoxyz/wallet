//! Shared test infrastructure for Tempo CLI integration tests.
//!
//! Provides mock servers, test configuration builders, assertion helpers,
//! and pre-built test fixtures so that each binary crate's integration
//! tests only need a thin `tests/common/mod.rs` re-export.

pub mod assert;
pub mod command;
pub mod fixture;
pub mod mock;

pub use assert::*;
pub use command::*;
pub use fixture::*;
pub use mock::*;
