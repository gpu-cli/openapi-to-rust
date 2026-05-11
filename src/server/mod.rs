//! Server codegen — opt-in Axum scaffolding for user-selected operations.
//!
//! Phase 1 (this file): read-only operation index for the
//! `openapi-to-rust server list` command. Later phases build on
//! [`OperationIndex`] for selector resolution, codegen, and TOML edits.
//!
//! See `docs/planning/server-codegen.md` for the full design.

use crate::analysis::{OperationInfo, SchemaAnalysis};

pub mod list;

/// Read-only snapshot of every operation in a spec, in the order the
/// analyzer surfaced them. Built once per command invocation.
#[derive(Debug, Clone)]
pub struct OperationIndex {
    operations: Vec<OperationSummary>,
}

/// Display-friendly subset of [`OperationInfo`] used by `server list`
/// and (later) by selector resolution and `server add`/`remove`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OperationSummary {
    pub operation_id: String,
    pub method: String,
    pub path: String,
    pub tags: Vec<String>,
    /// True when any response declares `text/event-stream`. Surfaces
    /// in the listing so users can spot SSE endpoints at a glance.
    pub supports_streaming: bool,
}

impl OperationIndex {
    /// Build an index from an already-completed [`SchemaAnalysis`].
    pub fn from_analysis(analysis: &SchemaAnalysis) -> Self {
        let operations = analysis
            .operations
            .values()
            .map(OperationSummary::from)
            .collect();
        Self { operations }
    }

    /// Build directly from pre-built summaries. Used by tests and by
    /// callers that have already projected `OperationInfo` to the
    /// display shape.
    pub fn from_summaries(operations: Vec<OperationSummary>) -> Self {
        Self { operations }
    }

    pub fn operations(&self) -> &[OperationSummary] {
        &self.operations
    }

    /// Count of distinct tags across all operations. Untagged ops are
    /// counted under a synthetic `<untagged>` bucket only for display;
    /// they do not appear in this count.
    pub fn tag_count(&self) -> usize {
        let mut tags: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for op in &self.operations {
            for t in &op.tags {
                tags.insert(t.as_str());
            }
        }
        tags.len()
    }
}

impl From<&OperationInfo> for OperationSummary {
    fn from(op: &OperationInfo) -> Self {
        Self {
            operation_id: op.operation_id.clone(),
            method: op.method.clone(),
            path: op.path.clone(),
            tags: op.tags.clone(),
            supports_streaming: op.supports_streaming,
        }
    }
}
