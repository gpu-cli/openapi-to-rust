//! Generated types from OpenAPI specification
//!
//! This file contains all the generated types for the API.
//! Do not edit manually - regenerate using the appropriate script.
#![allow(clippy::large_enum_variant)]
#![allow(clippy::format_in_format_args)]
#![allow(clippy::let_unit_value)]
#![allow(unreachable_patterns)]
use serde::{Deserialize, Serialize};
pub type ListTodosResponse = Vec<Todo>;
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
pub struct CreateTodoRequest {
    ///Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    ///The todo item title
    pub title: String,
}
impl CreateTodoRequest {
    /// Construct this request with every required wire field.
    pub fn new(title: String) -> Self {
        Self { title, description: None }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(title: String) -> CreateTodoRequestBuilder {
        CreateTodoRequestBuilder::new(title)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct CreateTodoRequestBuilder {
    value: CreateTodoRequest,
}
impl CreateTodoRequestBuilder {
    /// Start a builder with every required wire field.
    pub fn new(title: String) -> Self {
        Self {
            value: CreateTodoRequest::new(title),
        }
    }
    #[doc = concat!("Set the optional `", "description", "` request field.")]
    #[must_use]
    pub fn description(mut self, description: String) -> Self {
        self.value.description = Some(description);
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> CreateTodoRequest {
        self.value
    }
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
impl UpdateTodoRequest {
    /// Construct this request with every required wire field.
    pub fn new(completed: bool, title: String) -> Self {
        Self {
            completed,
            title,
            description: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(completed: bool, title: String) -> UpdateTodoRequestBuilder {
        UpdateTodoRequestBuilder::new(completed, title)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct UpdateTodoRequestBuilder {
    value: UpdateTodoRequest,
}
impl UpdateTodoRequestBuilder {
    /// Start a builder with every required wire field.
    pub fn new(completed: bool, title: String) -> Self {
        Self {
            value: UpdateTodoRequest::new(completed, title),
        }
    }
    #[doc = concat!("Set the optional `", "description", "` request field.")]
    #[must_use]
    pub fn description(mut self, description: String) -> Self {
        self.value.description = Some(description);
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> UpdateTodoRequest {
        self.value
    }
}
