//! Server codegen — opt-in Axum scaffolding for user-selected operations.
//!
//! Phase 1 (this file): read-only operation index for the
//! `openapi-to-rust server list` command. Later phases build on
//! [`OperationIndex`] for selector resolution, codegen, and TOML edits.
//!
//! See `docs/planning/server-codegen.md` for the full design.

use crate::analysis::{OperationInfo, SchemaAnalysis};

pub mod codegen;
pub mod edit;
pub mod list;
pub mod selector;

pub use selector::{Resolution, Selector, SelectorParseError, SelectorResolveError, resolve};

/// Parse and resolve raw operation selectors with enough context for config
/// errors. Shared by client and server generation.
pub fn resolve_operation_selectors(
    selectors: &[String],
    analysis: &SchemaAnalysis,
) -> Result<Resolution, OperationSelectionError> {
    let parsed = selectors
        .iter()
        .map(|selector| {
            Selector::parse(selector).map_err(|source| OperationSelectionError::Parse {
                selector: selector.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let index = OperationIndex::from_analysis(analysis);
    resolve(&parsed, &index).map_err(OperationSelectionError::Resolve)
}

/// Resolve a field whose contract is specifically one operation ID.
///
/// Unlike [`resolve_operation_selectors`], this does not interpret
/// `METHOD /path` or `tag:<name>` syntax. It still uses analyzer alias metadata
/// so renamed and ambiguous source IDs receive the same actionable errors.
pub fn resolve_operation_id(
    operation_id: &str,
    analysis: &SchemaAnalysis,
) -> Result<Resolution, OperationSelectionError> {
    let index = OperationIndex::from_analysis(analysis);
    resolve(&[Selector::OperationId(operation_id.to_string())], &index)
        .map_err(OperationSelectionError::Resolve)
}

#[derive(Debug, thiserror::Error)]
pub enum OperationSelectionError {
    #[error("invalid operation selector `{selector}`: {source}")]
    Parse {
        selector: String,
        #[source]
        source: SelectorParseError,
    },
    #[error("operation selector did not resolve: {0}")]
    Resolve(#[source] SelectorResolveError),
}

/// Read-only snapshot of every operation in a spec, in the order the
/// analyzer surfaced them. Built once per command invocation.
#[derive(Debug, Clone)]
pub struct OperationIndex {
    operations: Vec<OperationSummary>,
    operation_id_aliases: std::collections::BTreeMap<String, Vec<String>>,
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
        Self {
            operations,
            operation_id_aliases: analysis.operation_id_aliases.clone(),
        }
    }

    /// Build directly from pre-built summaries. Used by tests and by
    /// callers that have already projected `OperationInfo` to the
    /// display shape.
    pub fn from_summaries(operations: Vec<OperationSummary>) -> Self {
        Self {
            operations,
            operation_id_aliases: Default::default(),
        }
    }

    pub fn operations(&self) -> &[OperationSummary] {
        &self.operations
    }

    pub(crate) fn operation_id_aliases(&self, raw_id: &str) -> Option<&[String]> {
        self.operation_id_aliases.get(raw_id).map(Vec::as_slice)
    }

    #[cfg(test)]
    pub(crate) fn with_aliases(
        mut self,
        aliases: std::collections::BTreeMap<String, Vec<String>>,
    ) -> Self {
        self.operation_id_aliases = aliases;
        self
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
