//! HTTP client error types with comprehensive retry detection.
//!
//! This module provides error types for HTTP operations with built-in support for
//! retry detection, error categorization, and detailed error information.
//!
//! # Overview
//!
//! The [`HttpError`] enum covers all possible failure modes for HTTP requests:
//! - Network errors (connection failures, DNS issues)
//! - HTTP errors (4xx client errors, 5xx server errors)
//! - Serialization/deserialization errors
//! - Authentication errors
//! - Timeouts
//! - Configuration errors
//!
//! # Retry Detection
//!
//! The error type includes built-in retry detection via [`HttpError::is_retryable()`]:
//! - Network errors → retryable
//! - Timeouts → retryable
//! - 429 (rate limit) → retryable
//! - 500, 502, 503, 504 (server errors) → retryable
//! - All other errors → not retryable
//!
//! When using the generated HTTP client with retry middleware (reqwest-retry),
//! retryable errors are automatically retried with exponential backoff.
//!
//! # Examples
//!
//! ## Basic Error Handling
//!
//! ```
//! # use openapi_generator::http_error::HttpError;
//! fn handle_api_error(error: HttpError) {
//!     match error {
//!         HttpError::Network(e) => {
//!             eprintln!("Network error: {}", e);
//!             // Will be retried automatically if retry is configured
//!         }
//!         HttpError::Http { status, message, .. } => {
//!             match status {
//!                 400 => eprintln!("Bad request: {}", message),
//!                 401 => eprintln!("Unauthorized - check API key"),
//!                 404 => eprintln!("Not found"),
//!                 429 => eprintln!("Rate limited - will retry"),
//!                 500..=599 => eprintln!("Server error: {}", message),
//!                 _ => eprintln!("HTTP error {}: {}", status, message),
//!             }
//!         }
//!         HttpError::Timeout => {
//!             eprintln!("Request timeout - will retry");
//!         }
//!         e => {
//!             eprintln!("Other error: {}", e);
//!         }
//!     }
//! }
//! ```
//!
//! ## Retry Detection
//!
//! ```
//! # use openapi_generator::http_error::HttpError;
//! fn classify_error(error: &HttpError) {
//!     if error.is_retryable() {
//!         println!("Retryable error: {}", error);
//!         // If retry middleware is configured, this will be retried automatically
//!     } else if error.is_client_error() {
//!         println!("Client error (4xx): fix the request");
//!     } else if error.is_server_error() {
//!         println!("Server error (5xx): may be transient");
//!     } else {
//!         println!("Non-retryable error: {}", error);
//!     }
//! }
//! ```
//!
//! ## Creating Errors
//!
//! ```
//! use openapi_generator::http_error::HttpError;
//!
//! // Create HTTP error from status code
//! let error = HttpError::from_status(404, "Resource not found", None);
//!
//! // Create serialization error
//! let error = HttpError::serialization_error("invalid JSON");
//!
//! // Create deserialization error
//! let error = HttpError::deserialization_error("unexpected field");
//! ```
//!
//! # Integration with reqwest-retry
//!
//! When the generated HTTP client is configured with retry middleware,
//! the retry logic automatically handles retryable errors:
//!
//! ```toml
//! [http_client.retry]
//! max_retries = 3
//! initial_delay_ms = 500
//! max_delay_ms = 16000
//! ```
//!
//! The retry middleware uses exponential backoff and will retry:
//! - Network errors (connection failures)
//! - Timeouts
//! - HTTP 429 (rate limit)
//! - HTTP 500, 502, 503, 504 (server errors)
//!
//! # Error Categories
//!
//! Errors can be categorized using helper methods:
//! - [`HttpError::is_retryable()`] - Should this error be retried?
//! - [`HttpError::is_client_error()`] - Is this a 4xx error?
//! - [`HttpError::is_server_error()`] - Is this a 5xx error?

use thiserror::Error;

/// HTTP client errors that can occur during API requests
#[derive(Error, Debug)]
pub enum HttpError {
    /// Network or connection error
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// Request serialization error
    #[error("Failed to serialize request: {0}")]
    Serialization(String),

    /// Response deserialization error
    #[error("Failed to deserialize response: {0}")]
    Deserialization(String),

    /// HTTP error response (4xx, 5xx)
    #[error("HTTP error {status}: {message}")]
    Http {
        status: u16,
        message: String,
        body: Option<String>,
    },

    /// Authentication error
    #[error("Authentication error: {0}")]
    Auth(String),

    /// Request timeout
    #[error("Request timeout")]
    Timeout,

    /// Invalid configuration
    #[error("Configuration error: {0}")]
    Config(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl HttpError {
    /// Create an HTTP error from a status code and message
    pub fn from_status(status: u16, message: impl Into<String>, body: Option<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
            body,
        }
    }

    /// Create a serialization error
    pub fn serialization_error(error: impl std::fmt::Display) -> Self {
        Self::Serialization(error.to_string())
    }

    /// Create a deserialization error
    pub fn deserialization_error(error: impl std::fmt::Display) -> Self {
        Self::Deserialization(error.to_string())
    }

    /// Check if this is a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        matches!(self, Self::Http { status, .. } if *status >= 400 && *status < 500)
    }

    /// Check if this is a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        matches!(self, Self::Http { status, .. } if *status >= 500 && *status < 600)
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Timeout => true,
            Self::Http { status, .. } => {
                // Retry on 429 (rate limit), 500, 502, 503, 504
                matches!(status, 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }
}

/// Result type for HTTP operations
pub type HttpResult<T> = Result<T, HttpError>;
