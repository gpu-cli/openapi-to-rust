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
//! # use openapi_to_rust::http_error::HttpError;
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
//! # use openapi_to_rust::http_error::HttpError;
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
//! use openapi_to_rust::http_error::HttpError;
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

/// The generated validation-problem profile based on RFC 9457.
///
/// The distinctive module name prevents collisions with OpenAPI schemas named
/// `ProblemDetails` or `InvalidParameter`.
pub mod openapi_to_rust_problem {
    /// A sanitized validation problem emitted by generated servers.
    #[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
    pub struct ProblemDetails {
        #[serde(rename = "type")]
        pub type_uri: String,
        pub title: String,
        pub status: u16,
        pub code: String,
        #[serde(default)]
        pub errors: Vec<InvalidParameter>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub instance: Option<String>,
    }

    /// One safe, machine-readable request validation violation.
    #[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
    pub struct InvalidParameter {
        pub code: String,
        pub location: String,
        pub message: String,
    }
}

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

    /// A response body exceeded the configured in-memory limit
    #[error("Response body exceeded configured limit of {limit} bytes")]
    ResponseTooLarge { limit: usize },

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

/// Envelope for an API response we received but couldn't (or didn't) treat as success.
///
/// `ApiError<E>` is returned whenever the server actually responded — whether the
/// status was non-2xx, or the 2xx body failed to deserialize into the expected
/// type. `status`, `headers`, and `raw_body` preserve what the server actually
/// sent, while `body` is a convenient lossy UTF-8 rendering. `typed` is `Some(_)`
/// when the response body was successfully parsed into a
/// per-operation error type; `parse_error` records why parsing failed when not.
/// Formatting the error limits only the displayed body preview; the public
/// fields retain the complete response and parsing details.
#[derive(Debug, Clone)]
pub struct ApiError<E> {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: String,
    /// Exact response bytes before lossy UTF-8 conversion.
    pub raw_body: Vec<u8>,
    pub typed: Option<E>,
    pub parse_error: Option<String>,
}

const API_ERROR_BODY_DISPLAY_LIMIT: usize = 500;
const API_ERROR_BODY_TRUNCATION_MARKER: &str = "... [truncated]";

fn display_api_error_body(body: &str) -> std::borrow::Cow<'_, str> {
    let Some((end, _)) = body.char_indices().nth(API_ERROR_BODY_DISPLAY_LIMIT) else {
        return std::borrow::Cow::Borrowed(body);
    };

    let mut displayed = String::with_capacity(end + API_ERROR_BODY_TRUNCATION_MARKER.len());
    displayed.push_str(&body[..end]);
    displayed.push_str(API_ERROR_BODY_TRUNCATION_MARKER);
    std::borrow::Cow::Owned(displayed)
}

impl<E> ApiError<E> {
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }

    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    /// Decode the generated RFC 9457 validation-problem profile without
    /// disturbing a documented typed error.
    ///
    /// Only the `application/problem+json` media type is accepted. Arbitrary
    /// JSON error bodies are deliberately not reclassified as Problem Details.
    pub fn problem_details(&self) -> Option<openapi_to_rust_problem::ProblemDetails> {
        let content_type = self
            .headers
            .get(reqwest::header::CONTENT_TYPE)?
            .to_str()
            .ok()?;
        let media_type = content_type.split(';').next()?.trim();
        if !media_type.eq_ignore_ascii_case("application/problem+json") {
            return None;
        }
        serde_json::from_str(&self.body).ok()
    }
}

impl<E: std::fmt::Debug> std::fmt::Display for ApiError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "API error {}: {}",
            self.status,
            display_api_error_body(&self.body)
        )?;

        if let Some(typed) = &self.typed {
            write!(f, "; typed: {typed:?}")?;
        }

        if let Some(parse_error) = &self.parse_error {
            write!(f, "; parse error: {parse_error}")?;
        }

        Ok(())
    }
}

impl<E: std::fmt::Debug> std::error::Error for ApiError<E> {}

/// Result error type for generated operation methods.
///
/// `Transport` covers failures where the request never produced a response we
/// can inspect (network, timeout, middleware, request-side serialization).
/// `Api` covers any case where the server *did* respond — the envelope always
/// carries status + headers + raw body even when the typed deserialize fails.
#[derive(Debug, thiserror::Error)]
pub enum ApiOpError<E: std::fmt::Debug> {
    #[error(transparent)]
    Transport(#[from] HttpError),

    #[error(transparent)]
    Api(ApiError<E>),
}

impl<E: std::fmt::Debug> ApiOpError<E> {
    /// Convenience accessor: if this is an Api variant, return the envelope.
    pub fn api(&self) -> Option<&ApiError<E>> {
        match self {
            Self::Api(e) => Some(e),
            Self::Transport(_) => None,
        }
    }
}
