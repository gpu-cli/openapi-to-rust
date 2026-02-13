//! Generated HTTP client for regular API requests
//!
//! This file contains the HTTP client implementation for GET, POST, etc.
//! Do not edit manually - regenerate using the appropriate script.
use super::types::*;
use thiserror::Error;
/// HTTP client errors that can occur during API requests
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
    /// Response deserialization error
    #[error("Failed to deserialize response: {0}")]
    Deserialization(String),
    /// HTTP error response (4xx, 5xx)
    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String, body: Option<String> },
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
    pub fn from_status(
        status: u16,
        message: impl Into<String>,
        body: Option<String>,
    ) -> Self {
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
        matches!(self, Self::Http { status, .. } if * status >= 400 && * status < 500)
    }
    /// Check if this is a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        matches!(self, Self::Http { status, .. } if * status >= 500 && * status < 600)
    }
    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Middleware(_) => true,
            Self::Timeout => true,
            Self::Http { status, .. } => matches!(status, 429 | 500 | 502 | 503 | 504),
            _ => false,
        }
    }
}
/// Result type for HTTP operations
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
impl HttpClient {
    ///POST /todos
    pub async fn create_todo(&self, request: CreateTodoRequest) -> HttpResult<Todo> {
        let url = format!("{}{}", self.base_url, "/todos");
        let mut req = self
            .http_client
            .post(url)
            .body(
                serde_json::to_vec(&request)
                    .map_err(|e| HttpError::serialization_error(e))?,
            )
            .header("content-type", "application/json");
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        if status.is_success() {
            let body = response
                .json()
                .await
                .map_err(|e| HttpError::deserialization_error(e))?;
            Ok(body)
        } else {
            let status_code = status.as_u16();
            let message = status.canonical_reason().unwrap_or("Unknown error");
            let body = response.text().await.ok();
            Err(HttpError::from_status(status_code, message, body))
        }
    }
    ///DELETE /todos/{id}
    pub async fn delete_todo(&self) -> HttpResult<()> {
        let url = format!("{}{}", self.base_url, "/todos/{id}");
        let mut req = self.http_client.delete(url);
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        if status.is_success() {
            let body = response
                .json()
                .await
                .map_err(|e| HttpError::deserialization_error(e))?;
            Ok(body)
        } else {
            let status_code = status.as_u16();
            let message = status.canonical_reason().unwrap_or("Unknown error");
            let body = response.text().await.ok();
            Err(HttpError::from_status(status_code, message, body))
        }
    }
    ///GET /todos/{id}
    pub async fn get_todo(&self) -> HttpResult<Todo> {
        let url = format!("{}{}", self.base_url, "/todos/{id}");
        let mut req = self.http_client.get(url);
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        if status.is_success() {
            let body = response
                .json()
                .await
                .map_err(|e| HttpError::deserialization_error(e))?;
            Ok(body)
        } else {
            let status_code = status.as_u16();
            let message = status.canonical_reason().unwrap_or("Unknown error");
            let body = response.text().await.ok();
            Err(HttpError::from_status(status_code, message, body))
        }
    }
    ///GET /todos
    pub async fn list_todos(&self) -> HttpResult<()> {
        let url = format!("{}{}", self.base_url, "/todos");
        let mut req = self.http_client.get(url);
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        if status.is_success() {
            let body = response
                .json()
                .await
                .map_err(|e| HttpError::deserialization_error(e))?;
            Ok(body)
        } else {
            let status_code = status.as_u16();
            let message = status.canonical_reason().unwrap_or("Unknown error");
            let body = response.text().await.ok();
            Err(HttpError::from_status(status_code, message, body))
        }
    }
    ///PUT /todos/{id}
    pub async fn update_todo(&self, request: UpdateTodoRequest) -> HttpResult<Todo> {
        let url = format!("{}{}", self.base_url, "/todos/{id}");
        let mut req = self
            .http_client
            .put(url)
            .body(
                serde_json::to_vec(&request)
                    .map_err(|e| HttpError::serialization_error(e))?,
            )
            .header("content-type", "application/json");
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        for (name, value) in &self.custom_headers {
            req = req.header(name, value);
        }
        let response = req.send().await?;
        let status = response.status();
        if status.is_success() {
            let body = response
                .json()
                .await
                .map_err(|e| HttpError::deserialization_error(e))?;
            Ok(body)
        } else {
            let status_code = status.as_u16();
            let message = status.canonical_reason().unwrap_or("Unknown error");
            let body = response.text().await.ok();
            Err(HttpError::from_status(status_code, message, body))
        }
    }
}
