use std::collections::BTreeMap;

/// Configuration for SSE streaming client generation
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// List of streaming endpoints to generate clients for
    pub endpoints: Vec<StreamingEndpoint>,
    /// Whether to generate streaming client implementations
    pub generate_client: bool,
    /// Name of the generated streaming client module
    pub client_module_name: String,
    /// Whether to generate event parsing helper utilities
    pub event_parser_helpers: bool,
    /// Configuration for automatic reconnection logic
    pub reconnection_config: Option<ReconnectionConfig>,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            endpoints: Vec::new(),
            generate_client: true,
            client_module_name: "streaming".to_string(),
            event_parser_helpers: true,
            reconnection_config: None,
        }
    }
}

/// HTTP method for streaming endpoint
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum HttpMethod {
    #[default]
    Post,
    Get,
}

/// Configuration for a single streaming endpoint
#[derive(Debug, Clone)]
pub struct StreamingEndpoint {
    /// OpenAPI operation ID that supports streaming
    pub operation_id: String,
    /// URL path for the endpoint (e.g., "/global/event")
    pub path: String,
    /// HTTP method for the streaming request (default: POST)
    pub http_method: HttpMethod,
    /// Parameter name that controls streaming (e.g., "stream"), empty for always-streaming endpoints
    /// Only used for POST requests
    pub stream_parameter: String,
    /// Query parameters for GET requests (e.g., session_id)
    pub query_parameters: Vec<QueryParameter>,
    /// Name of the union type that represents streaming events
    pub event_union_type: String,
    /// Content type for streaming responses (e.g., "text/event-stream")
    pub content_type: Option<String>,
    /// Base URL for the API (optional override)
    pub base_url: Option<String>,
    /// Event flow pattern for this endpoint
    pub event_flow: EventFlow,
    /// Required headers for streaming requests
    pub required_headers: Vec<(String, String)>,
    /// Authentication header configuration
    pub auth_header: Option<AuthHeader>,
    /// Optional headers that can be set dynamically (e.g., beta features)
    pub optional_headers: Vec<OptionalHeader>,
}

/// Query parameter configuration for GET streaming endpoints
#[derive(Debug, Clone)]
pub struct QueryParameter {
    /// Parameter name
    pub name: String,
    /// Whether this parameter is required
    pub required: bool,
}

impl Default for StreamingEndpoint {
    fn default() -> Self {
        Self {
            operation_id: String::new(),
            path: String::new(),
            http_method: HttpMethod::default(),
            stream_parameter: String::new(),
            query_parameters: Vec::new(),
            event_union_type: String::new(),
            content_type: Some("text/event-stream".to_string()),
            base_url: None,
            event_flow: EventFlow::Simple,
            required_headers: Vec::new(),
            auth_header: None,
            optional_headers: Vec::new(),
        }
    }
}

/// Authentication header configuration
#[derive(Debug, Clone)]
pub enum AuthHeader {
    /// Bearer token format: "Authorization: Bearer {token}"
    Bearer(String),
    /// Direct API key format: "{header_name}: {api_key}"
    ApiKey(String),
}

/// Defines the event flow pattern for streaming
#[derive(Debug, Clone, Default)]
pub enum EventFlow {
    /// Simple streaming - just emit events as they arrive
    #[default]
    Simple,
    /// Start-Delta-Stop pattern with specific event types
    StartDeltaStop {
        /// Events that signal the start of a stream
        start_events: Vec<String>,
        /// Events that contain incremental updates
        delta_events: Vec<String>,
        /// Events that signal the end of a stream
        stop_events: Vec<String>,
    },
}

/// Configuration for automatic reconnection logic
#[derive(Debug, Clone)]
pub struct ReconnectionConfig {
    /// Maximum number of reconnection attempts
    pub max_retries: u32,
    /// Initial delay between reconnection attempts (milliseconds)
    pub initial_delay_ms: u64,
    /// Maximum delay between reconnection attempts (milliseconds)
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

/// Error types for streaming operations
#[derive(Debug, Clone)]
pub enum StreamingError {
    /// Connection error
    Connection(String),
    /// Parsing error for SSE events
    Parsing(String),
    /// Authentication error
    Authentication(String),
    /// Rate limiting error
    RateLimit(String),
    /// An error response exceeded the configured in-memory limit
    ResponseTooLarge { limit: usize },
    /// Generic API error
    Api(String),
}

impl std::fmt::Display for StreamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamingError::Connection(msg) => write!(f, "Connection error: {msg}"),
            StreamingError::Parsing(msg) => write!(f, "Parsing error: {msg}"),
            StreamingError::Authentication(msg) => write!(f, "Authentication error: {msg}"),
            StreamingError::RateLimit(msg) => write!(f, "Rate limit error: {msg}"),
            StreamingError::ResponseTooLarge { limit } => {
                write!(
                    f,
                    "Response body exceeded configured limit of {limit} bytes"
                )
            }
            StreamingError::Api(msg) => write!(f, "API error: {msg}"),
        }
    }
}

impl std::error::Error for StreamingError {}

/// Configuration for optional headers that can be set dynamically
#[derive(Debug, Clone)]
pub struct OptionalHeader {
    /// Header name
    pub name: String,
    /// Description of what this header does
    pub description: String,
    /// Whether this header can accept multiple values (comma-separated)
    pub multiple_values: bool,
    /// Default value if any
    pub default_value: Option<String>,
    /// Example values for documentation
    pub examples: Vec<String>,
}

/// Configuration for detecting streaming endpoints from OpenAPI specs
#[derive(Debug, Clone)]
pub struct StreamingDetectionConfig {
    /// Parameter names that indicate streaming capability
    pub stream_parameter_names: Vec<String>,
    /// Content types that indicate SSE responses
    pub sse_content_types: Vec<String>,
    /// Schema name patterns for event types
    pub event_type_patterns: Vec<String>,
}

impl Default for StreamingDetectionConfig {
    fn default() -> Self {
        Self {
            stream_parameter_names: vec!["stream".to_string()],
            sse_content_types: vec!["text/event-stream".to_string()],
            event_type_patterns: vec![
                "*Event".to_string(),
                "*StreamEvent".to_string(),
                "*StreamResponse".to_string(),
            ],
        }
    }
}

/// Detected streaming endpoint from OpenAPI analysis
#[derive(Debug, Clone)]
pub struct DetectedStreamingEndpoint {
    /// Operation ID
    pub operation_id: String,
    /// Stream parameter name
    pub stream_parameter: String,
    /// Detected event union type
    pub event_union_type: Option<String>,
    /// Detected content type
    pub content_type: Option<String>,
    /// Detected event types
    pub event_types: Vec<String>,
}

/// Auto-detection result for streaming capabilities
#[derive(Debug, Clone)]
pub struct StreamingDetectionResult {
    /// Detected streaming endpoints
    pub endpoints: Vec<DetectedStreamingEndpoint>,
    /// Event types found in the spec
    pub event_types: BTreeMap<String, Vec<String>>,
    /// Discriminated unions that could be streaming events
    pub potential_event_unions: Vec<String>,
}
