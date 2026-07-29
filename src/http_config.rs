//! Runtime HTTP client configuration types
//!
//! These types are used by generated code at runtime and represent the actual
//! configuration that will be used by the HTTP client.

use std::collections::HashMap;

/// Runtime HTTP client configuration (used by generated code)
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Base URL for all API requests
    pub base_url: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: Option<u64>,
    /// Maximum response-body bytes buffered in memory
    pub max_response_body_bytes: Option<usize>,
    /// Default headers to include in all requests
    pub default_headers: HashMap<String, String>,
}

/// Retry configuration for HTTP requests
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay in milliseconds before first retry
    pub initial_delay_ms: u64,
    /// Maximum delay in milliseconds between retries
    pub max_delay_ms: u64,
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub enum AuthConfig {
    /// Bearer token authentication (e.g., "Authorization: Bearer TOKEN")
    Bearer {
        /// Header name for the bearer token (default: "Authorization")
        header_name: String,
    },
    /// API key authentication (e.g., "X-API-Key: YOUR_KEY")
    ApiKey {
        /// Header name for the API key
        header_name: String,
    },
    /// Custom authentication with configurable header and prefix
    Custom {
        /// Header name for the authentication token
        header_name: String,
        /// Optional prefix for the header value (e.g., "Bearer ")
        header_value_prefix: Option<String>,
    },
}
