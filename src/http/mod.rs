//! HTTP client and request handling.
//!
//! Provides [`HttpClient`] for building reqwest clients, executing
//! requests with retry logic, and managing runtime configuration.

mod client;
mod response;

pub(crate) use client::{HttpClient, HttpRequestPlan};
pub(crate) use response::{http_status_text, HttpResponse};
