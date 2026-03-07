//! HTTP client and request handling.
//!
//! Provides [`HttpClient`] for building reqwest clients, executing
//! requests with retry logic, and managing runtime configuration.

mod client;
mod fmt;
mod response;

pub use client::{HttpClient, HttpRequestPlan, DEFAULT_USER_AGENT};
pub use fmt::{format_http_error, print_headers};
pub use response::{headers_from_reqwest, HttpResponse};
