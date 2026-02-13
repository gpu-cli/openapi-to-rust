use openapi_generator::http_error::{HttpError, HttpResult};

fn main() {
    println!("=== HTTP Error Handling Examples ===\n");

    // Example 1: HTTP error with status code
    let not_found = HttpError::from_status(
        404,
        "Resource not found",
        Some("The requested resource does not exist".to_string()),
    );
    println!("1. HTTP Error:");
    println!("   Error: {}", not_found);
    println!("   Is client error: {}", not_found.is_client_error());
    println!("   Is retryable: {}", not_found.is_retryable());
    println!();

    // Example 2: Retryable server error
    let server_error = HttpError::from_status(503, "Service Unavailable", None);
    println!("2. Retryable Server Error:");
    println!("   Error: {}", server_error);
    println!("   Is server error: {}", server_error.is_server_error());
    println!("   Is retryable: {}", server_error.is_retryable());
    println!();

    // Example 3: Rate limit error (retryable)
    let rate_limit = HttpError::from_status(429, "Too Many Requests", None);
    println!("3. Rate Limit Error:");
    println!("   Error: {}", rate_limit);
    println!("   Is retryable: {}", rate_limit.is_retryable());
    println!();

    // Example 4: Serialization error
    let ser_error = HttpError::serialization_error("Failed to serialize request body");
    println!("4. Serialization Error:");
    println!("   Error: {}", ser_error);
    println!("   Is retryable: {}", ser_error.is_retryable());
    println!();

    // Example 5: Authentication error
    let auth_error = HttpError::Auth("Invalid API key".to_string());
    println!("5. Authentication Error:");
    println!("   Error: {}", auth_error);
    println!("   Is retryable: {}", auth_error.is_retryable());
    println!();

    // Example 6: Timeout error
    let timeout = HttpError::Timeout;
    println!("6. Timeout Error:");
    println!("   Error: {}", timeout);
    println!("   Is retryable: {}", timeout.is_retryable());
    println!();

    // Example 7: Using HttpResult type
    let success: HttpResult<String> = Ok("Success!".to_string());
    let failure: HttpResult<String> = Err(HttpError::Timeout);

    println!("7. Using HttpResult:");
    println!("   Success: {:?}", success);
    println!("   Failure: {:?}", failure);
    println!();

    // Example 8: Error handling with match
    println!("8. Error Classification:");
    let errors = vec![
        HttpError::from_status(400, "Bad Request", None),
        HttpError::from_status(500, "Internal Server Error", None),
        HttpError::Timeout,
        HttpError::Auth("Invalid token".to_string()),
    ];

    for error in errors {
        let classification = if error.is_client_error() {
            "Client Error (4xx)"
        } else if error.is_server_error() {
            "Server Error (5xx)"
        } else if error.is_retryable() {
            "Retryable Error"
        } else {
            "Non-retryable Error"
        };
        println!("   {} -> {}", error, classification);
    }
}
