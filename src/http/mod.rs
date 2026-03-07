//! HTTP client and request handling.
//!
//! Provides [`HttpClient`] for building reqwest clients, executing
//! requests with retry logic, and managing runtime configuration.

mod client;
mod response;

pub(crate) use client::{HttpClient, HttpRequestPlan};
pub(crate) use response::{extract_headers, http_status_text, print_headers, HttpResponse};
