//! Generated types from OpenAPI specification
//!
//! This file contains all the generated types for the API.
//! Do not edit manually - regenerate using the appropriate script.
#![allow(clippy::large_enum_variant)]
#![allow(unreachable_patterns)]
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateTodoRequest {
    ///Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    ///The todo item title
    pub title: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Todo {
    ///Whether the todo is completed
    #[serde(default)]
    pub completed: bool,
    ///Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    ///Unique identifier for the todo
    pub id: String,
    ///The todo item title
    pub title: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateTodoRequest {
    ///Whether the todo is completed
    pub completed: bool,
    ///Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    ///The todo item title
    pub title: String,
}
