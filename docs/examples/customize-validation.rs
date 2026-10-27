use crate::generated::ProblemDetails;
use axum::{
    body::{to_bytes, Body},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

pub async fn customize_validation(response: Response) -> Response {
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        != Some("application/problem+json")
    {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    // Only buffer bounded problem bodies, never normal or streaming responses.
    let Ok(bytes) = to_bytes(body, 64 * 1024).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut output = bytes.to_vec();
    if let Ok(mut problem) = serde_json::from_slice::<ProblemDetails>(&bytes) {
        if let Some(slug) = problem
            .r#type
            .strip_prefix("https://openapi-to-rust.dev/problems/")
        {
            problem.r#type = format!("https://api.example.com/problems/{slug}");
            if let Ok(encoded) = serde_json::to_vec(&problem) {
                output = encoded;
                // The new JSON may have a different byte length.
                parts.headers.remove(header::CONTENT_LENGTH);
            }
        }
    }
    Response::from_parts(parts, Body::from(output))
}
