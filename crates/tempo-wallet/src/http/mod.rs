//! HTTP client and request handling.
//!
//! Provides [`HttpClient`] for building reqwest clients, executing
//! requests with retry logic, and managing runtime configuration.

mod client;
mod fmt;
mod response;

pub(crate) use client::{HttpClient, HttpRequestPlan, DEFAULT_USER_AGENT};
pub(crate) use fmt::{format_http_error, print_headers};
pub(crate) use response::{headers_from_reqwest, HttpResponse};
