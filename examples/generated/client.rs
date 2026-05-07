//! Generated HTTP client for regular API requests
//!
//! This file contains the HTTP client implementation for GET, POST, etc.
//! Do not edit manually - regenerate using the appropriate script.
#![allow(clippy::format_in_format_args)]
#![allow(clippy::let_unit_value)]
use super::types::*;
use thiserror::Error;
/// Transport-level errors: failures where we never received an
/// inspectable HTTP response from the server.
///
/// HTTP responses with non-2xx status codes are surfaced as
/// [`ApiError`] inside [`ApiOpError::Api`], not here, so callers can
/// always inspect status, headers, and the raw body when the server
/// actually responded.
#[derive(Error, Debug)]
pub enum HttpError {
    /// Network or connection error (from reqwest)
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    /// Middleware error (from reqwest-middleware)
    #[error("Middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    /// Request serialization error
    #[error("Failed to serialize request: {0}")]
    Serialization(String),
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
    /// Create a serialization error
    pub fn serialization_error(error: impl std::fmt::Display) -> Self {
        Self::Serialization(error.to_string())
    }
    /// Check if this transport error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Middleware(_) | Self::Timeout)
    }
}
/// Envelope returned for any HTTP response that we received but
/// couldn't (or didn't) treat as a successful typed result.
///
/// Includes both non-2xx responses and 2xx responses whose body
/// failed to deserialize into the expected success type. `status`,
/// `headers`, and `body` are always populated so callers can
/// inspect what the server sent without modifying the generated
/// code. `typed` carries the parsed per-operation error variant
/// when the body matched a declared schema.
#[derive(Debug, Clone)]
pub struct ApiError<E> {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: String,
    pub typed: Option<E>,
    pub parse_error: Option<String>,
}
impl<E> ApiError<E> {
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }
    /// Retry guidance for the response. Mirrors the previous
    /// HttpError logic for backwards-compatible retry middleware.
    pub fn is_retryable(&self) -> bool {
        matches!(self.status, 429 | 500 | 502 | 503 | 504)
    }
}
impl<E: std::fmt::Debug> std::fmt::Display for ApiError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API error {}: {}", self.status, self.body)
    }
}
impl<E: std::fmt::Debug> std::error::Error for ApiError<E> {}
/// Result error type returned by every generated operation method.
///
/// `Transport` covers failures where we never got an inspectable
/// response (network, timeout, middleware, request-side
/// serialization). `Api` covers any case where the server *did*
/// respond — the envelope always carries status + headers + raw
/// body even when the typed deserialize fails.
#[derive(Debug, Error)]
pub enum ApiOpError<E: std::fmt::Debug> {
    #[error(transparent)]
    Transport(#[from] HttpError),
    #[error(transparent)]
    Api(ApiError<E>),
}
impl<E: std::fmt::Debug> ApiOpError<E> {
    /// Returns the API envelope when this is an `Api` variant.
    pub fn api(&self) -> Option<&ApiError<E>> {
        match self {
            Self::Api(e) => Some(e),
            Self::Transport(_) => None,
        }
    }
    /// True when the underlying error came from the server (i.e.
    /// any `Api` variant) rather than the transport layer.
    pub fn is_api_error(&self) -> bool {
        matches!(self, Self::Api(_))
    }
}
impl<E: std::fmt::Debug> From<reqwest::Error> for ApiOpError<E> {
    fn from(e: reqwest::Error) -> Self {
        Self::Transport(HttpError::Network(e))
    }
}
impl<E: std::fmt::Debug> From<reqwest_middleware::Error> for ApiOpError<E> {
    fn from(e: reqwest_middleware::Error) -> Self {
        Self::Transport(HttpError::Middleware(e))
    }
}
/// Result alias for transport-only error paths (e.g. helpers that
/// don't have a per-operation error type). Generated operation
/// methods use [`ApiOpError`] directly.
pub type HttpResult<T> = Result<T, HttpError>;
/// Retry configuration for HTTP requests
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}
impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }
    }
}
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use std::collections::BTreeMap;
/// HTTP client for making API requests
#[derive(Clone)]
pub struct HttpClient {
    base_url: String,
    api_key: Option<String>,
    http_client: ClientWithMiddleware,
    custom_headers: BTreeMap<String, String>,
}
impl HttpClient {
    /// Create a new HTTP client with default configuration
    pub fn new() -> Self {
        Self::with_config(None, true)
    }
    /// Create a new HTTP client with custom configuration
    pub fn with_config(retry_config: Option<RetryConfig>, enable_tracing: bool) -> Self {
        let reqwest_client = reqwest::Client::new();
        let mut client_builder = ClientBuilder::new(reqwest_client);
        if enable_tracing {
            use reqwest_tracing::TracingMiddleware;
            client_builder = client_builder.with(TracingMiddleware::default());
        }
        if let Some(config) = retry_config {
            use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
            let retry_policy = ExponentialBackoff::builder()
                .retry_bounds(
                    std::time::Duration::from_millis(config.initial_delay_ms),
                    std::time::Duration::from_millis(config.max_delay_ms),
                )
                .build_with_max_retries(config.max_retries);
            let retry_middleware = RetryTransientMiddleware::new_with_policy(
                retry_policy,
            );
            client_builder = client_builder.with(retry_middleware);
        }
        let http_client = client_builder.build();
        Self {
            base_url: String::new(),
            api_key: None,
            http_client,
            custom_headers: BTreeMap::new(),
        }
    }
    /// Set the base URL for all requests
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
    /// Set the API key for authentication
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }
    /// Add a custom header to all requests
    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.custom_headers.insert(name.into(), value.into());
        self
    }
    /// Add multiple custom headers
    pub fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.custom_headers.extend(headers);
        self
    }
}
impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}
impl HttpClient {
    ///POST /todos
    pub async fn create_todo(
        &self,
        request: CreateTodoRequest,
    ) -> Result<Todo, ApiOpError<serde_json::Value>> {
        let request_url = format!("{}{}", self.base_url, "/todos");
        let mut req = self
            .http_client
            .post(request_url)
            .body(serde_json::to_vec(&request).map_err(HttpError::serialization_error)?)
            .header("content-type", "application/json");
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        let status_code = status.as_u16();
        let headers = response.headers().clone();
        let body_text = response
            .text()
            .await
            .map_err(|e| ApiOpError::Transport(HttpError::Network(e)))?;
        if status.is_success() {
            match serde_json::from_str(&body_text) {
                Ok(body) => Ok(body),
                Err(e) => {
                    Err(
                        ApiOpError::Api(ApiError {
                            status: status_code,
                            headers: headers,
                            body: body_text,
                            typed: None,
                            parse_error: Some(
                                format!("failed to deserialize 2xx response body: {}", e),
                            ),
                        }),
                    )
                }
            }
        } else {
            let typed: Option<serde_json::Value>;
            let parse_error: Option<String>;
            match status_code {
                _ => {
                    match serde_json::from_str::<serde_json::Value>(&body_text) {
                        Ok(v) => {
                            typed = Some(v);
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
            Err(
                ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    typed,
                    parse_error,
                }),
            )
        }
    }
    ///DELETE /todos/{id}
    pub async fn delete_todo(
        &self,
        id: impl AsRef<str>,
    ) -> Result<(), ApiOpError<serde_json::Value>> {
        let request_url = format!(
            "{}{}", self.base_url, format!("/todos/{}", id.as_ref())
        );
        let mut req = self.http_client.delete(request_url);
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        let status_code = status.as_u16();
        let headers = response.headers().clone();
        let body_text = response
            .text()
            .await
            .map_err(|e| ApiOpError::Transport(HttpError::Network(e)))?;
        if status.is_success() {
            let _ = body_text;
            let _ = headers;
            Ok(())
        } else {
            let typed: Option<serde_json::Value>;
            let parse_error: Option<String>;
            match status_code {
                _ => {
                    match serde_json::from_str::<serde_json::Value>(&body_text) {
                        Ok(v) => {
                            typed = Some(v);
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
            Err(
                ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    typed,
                    parse_error,
                }),
            )
        }
    }
    ///GET /todos/{id}
    pub async fn get_todo(
        &self,
        id: impl AsRef<str>,
    ) -> Result<Todo, ApiOpError<serde_json::Value>> {
        let request_url = format!(
            "{}{}", self.base_url, format!("/todos/{}", id.as_ref())
        );
        let mut req = self.http_client.get(request_url);
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        let status_code = status.as_u16();
        let headers = response.headers().clone();
        let body_text = response
            .text()
            .await
            .map_err(|e| ApiOpError::Transport(HttpError::Network(e)))?;
        if status.is_success() {
            match serde_json::from_str(&body_text) {
                Ok(body) => Ok(body),
                Err(e) => {
                    Err(
                        ApiOpError::Api(ApiError {
                            status: status_code,
                            headers: headers,
                            body: body_text,
                            typed: None,
                            parse_error: Some(
                                format!("failed to deserialize 2xx response body: {}", e),
                            ),
                        }),
                    )
                }
            }
        } else {
            let typed: Option<serde_json::Value>;
            let parse_error: Option<String>;
            match status_code {
                _ => {
                    match serde_json::from_str::<serde_json::Value>(&body_text) {
                        Ok(v) => {
                            typed = Some(v);
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
            Err(
                ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    typed,
                    parse_error,
                }),
            )
        }
    }
    ///GET /todos
    pub async fn list_todos(
        &self,
    ) -> Result<ListTodosResponse, ApiOpError<serde_json::Value>> {
        let request_url = format!("{}{}", self.base_url, "/todos");
        let mut req = self.http_client.get(request_url);
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        let status_code = status.as_u16();
        let headers = response.headers().clone();
        let body_text = response
            .text()
            .await
            .map_err(|e| ApiOpError::Transport(HttpError::Network(e)))?;
        if status.is_success() {
            match serde_json::from_str(&body_text) {
                Ok(body) => Ok(body),
                Err(e) => {
                    Err(
                        ApiOpError::Api(ApiError {
                            status: status_code,
                            headers: headers,
                            body: body_text,
                            typed: None,
                            parse_error: Some(
                                format!("failed to deserialize 2xx response body: {}", e),
                            ),
                        }),
                    )
                }
            }
        } else {
            let typed: Option<serde_json::Value>;
            let parse_error: Option<String>;
            match status_code {
                _ => {
                    match serde_json::from_str::<serde_json::Value>(&body_text) {
                        Ok(v) => {
                            typed = Some(v);
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
            Err(
                ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    typed,
                    parse_error,
                }),
            )
        }
    }
    ///PUT /todos/{id}
    pub async fn update_todo(
        &self,
        id: impl AsRef<str>,
        request: UpdateTodoRequest,
    ) -> Result<Todo, ApiOpError<serde_json::Value>> {
        let request_url = format!(
            "{}{}", self.base_url, format!("/todos/{}", id.as_ref())
        );
        let mut req = self
            .http_client
            .put(request_url)
            .body(serde_json::to_vec(&request).map_err(HttpError::serialization_error)?)
            .header("content-type", "application/json");
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        let status_code = status.as_u16();
        let headers = response.headers().clone();
        let body_text = response
            .text()
            .await
            .map_err(|e| ApiOpError::Transport(HttpError::Network(e)))?;
        if status.is_success() {
            match serde_json::from_str(&body_text) {
                Ok(body) => Ok(body),
                Err(e) => {
                    Err(
                        ApiOpError::Api(ApiError {
                            status: status_code,
                            headers: headers,
                            body: body_text,
                            typed: None,
                            parse_error: Some(
                                format!("failed to deserialize 2xx response body: {}", e),
                            ),
                        }),
                    )
                }
            }
        } else {
            let typed: Option<serde_json::Value>;
            let parse_error: Option<String>;
            match status_code {
                _ => {
                    match serde_json::from_str::<serde_json::Value>(&body_text) {
                        Ok(v) => {
                            typed = Some(v);
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
            Err(
                ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    typed,
                    parse_error,
                }),
            )
        }
    }
}
