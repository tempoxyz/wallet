//! Passkey authentication module for Tempo wallet integration.
//!
//! This module provides functionality for authenticating with passkey-based
//! Tempo wallets via the presto.tempo.xyz OAuth flow.
//!
//! # Module Structure
//!
//! - `config` - AccessKey and PasskeyConfig types
//! - `auth_server` - Local HTTP server for OAuth callbacks
//! - `browser` - Browser opening utilities

mod auth_server;
mod browser;
mod config;

pub use auth_server::{AuthServer, CallbackPayload};
pub use browser::open_browser;
pub use config::{AccessKey, PasskeyConfig};
