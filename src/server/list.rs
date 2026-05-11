//! `openapi-to-rust server list` — read-only operation discovery.
//!
//! Filters compose: each non-empty filter narrows the result set.
//! Output is either an aligned table (default) or a JSON array.

use super::{OperationIndex, OperationSummary};

/// Filter criteria. Empty filters match everything.
#[derive(Debug, Default, Clone)]
pub struct ListFilter {
    pub tag: Option<String>,
    pub method: Option<String>,
    pub grep: Option<String>,
}

impl ListFilter {
    fn matches(&self, op: &OperationSummary) -> bool {
        if let Some(tag) = &self.tag {
            let needle = tag.to_ascii_lowercase();
            if !op
                .tags
                .iter()
                .any(|t| t.to_ascii_lowercase().contains(&needle))
            {
                return false;
            }
        }
        if let Some(method) = &self.method
            && !op.method.eq_ignore_ascii_case(method)
        {
            return false;
        }
        if let Some(grep) = &self.grep {
            let needle = grep.to_ascii_lowercase();
            let hay_id = op.operation_id.to_ascii_lowercase();
            let hay_path = op.path.to_ascii_lowercase();
            if !hay_id.contains(&needle) && !hay_path.contains(&needle) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ListOutput {
    Table,
    Json,
}

/// Apply filters and return the matching operations in index order.
pub fn select<'a>(index: &'a OperationIndex, filter: &ListFilter) -> Vec<&'a OperationSummary> {
    index
        .operations()
        .iter()
        .filter(|op| filter.matches(op))
        .collect()
}

/// Render selected operations as either an aligned table or JSON.
/// Returns the rendered string and the count of matched operations.
pub fn render(index: &OperationIndex, filter: &ListFilter, output: ListOutput) -> (String, usize) {
    let matched = select(index, filter);
    let count = matched.len();
    let body = match output {
        ListOutput::Table => render_table(&matched, index.tag_count()),
        ListOutput::Json => render_json(&matched),
    };
    (body, count)
}

fn render_json(matched: &[&OperationSummary]) -> String {
    serde_json::to_string_pretty(matched).unwrap_or_else(|_| "[]".to_string())
}

fn render_table(matched: &[&OperationSummary], total_tags: usize) -> String {
    if matched.is_empty() {
        return "No operations match the given filters.\n".to_string();
    }

    let tag_w = matched
        .iter()
        .map(|op| display_tag(op).len())
        .max()
        .unwrap_or(3)
        .max("TAG".len());
    let id_w = matched
        .iter()
        .map(|op| display_op_id(op).len())
        .max()
        .unwrap_or(5)
        .max("OP_ID".len());
    let method_w = matched
        .iter()
        .map(|op| op.method.len())
        .max()
        .unwrap_or(6)
        .max("METHOD".len());

    let mut out = String::new();
    use std::fmt::Write;
    let _ = writeln!(
        out,
        "{:<tag_w$}  {:<id_w$}  {:<method_w$}  PATH",
        "TAG",
        "OP_ID",
        "METHOD",
        tag_w = tag_w,
        id_w = id_w,
        method_w = method_w,
    );
    for op in matched {
        let id = display_op_id(op);
        let tag = display_tag(op);
        let stream_marker = if op.supports_streaming { "  [SSE]" } else { "" };
        let _ = writeln!(
            out,
            "{:<tag_w$}  {:<id_w$}  {:<method_w$}  {}{}",
            tag,
            id,
            op.method,
            op.path,
            stream_marker,
            tag_w = tag_w,
            id_w = id_w,
            method_w = method_w,
        );
    }
    let _ = writeln!(
        out,
        "\n{} operation{} across {} tag{}.",
        matched.len(),
        if matched.len() == 1 { "" } else { "s" },
        total_tags,
        if total_tags == 1 { "" } else { "s" },
    );
    out
}

fn display_tag(op: &OperationSummary) -> String {
    if op.tags.is_empty() {
        "<untagged>".to_string()
    } else {
        op.tags.join(",")
    }
}

fn display_op_id(op: &OperationSummary) -> String {
    if op.operation_id.is_empty() {
        "-".to_string()
    } else {
        op.operation_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, method: &str, path: &str, tags: &[&str], sse: bool) -> OperationSummary {
        OperationSummary {
            operation_id: id.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            supports_streaming: sse,
        }
    }

    fn index_from(ops: Vec<OperationSummary>) -> OperationIndex {
        OperationIndex::from_summaries(ops)
    }

    #[test]
    fn empty_filter_returns_all() {
        let idx = index_from(vec![
            op("a", "GET", "/a", &["X"], false),
            op("b", "POST", "/b", &["Y"], true),
        ]);
        assert_eq!(select(&idx, &ListFilter::default()).len(), 2);
    }

    #[test]
    fn tag_filter_is_case_insensitive_substring() {
        let idx = index_from(vec![
            op("a", "GET", "/a", &["Chat"], false),
            op("b", "POST", "/b", &["Embeddings"], false),
        ]);
        let f = ListFilter {
            tag: Some("chat".to_string()),
            ..Default::default()
        };
        let got = select(&idx, &f);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].operation_id, "a");
    }

    #[test]
    fn method_filter_exact() {
        let idx = index_from(vec![
            op("a", "GET", "/a", &[], false),
            op("b", "POST", "/b", &[], false),
        ]);
        let f = ListFilter {
            method: Some("post".to_string()),
            ..Default::default()
        };
        let got = select(&idx, &f);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].operation_id, "b");
    }

    #[test]
    fn grep_matches_op_id_or_path() {
        let idx = index_from(vec![
            op("createResponse", "POST", "/v1/responses", &[], true),
            op("listFiles", "GET", "/v1/files", &[], false),
        ]);
        let f = ListFilter {
            grep: Some("response".to_string()),
            ..Default::default()
        };
        assert_eq!(select(&idx, &f).len(), 1);
        let f = ListFilter {
            grep: Some("/v1/files".to_string()),
            ..Default::default()
        };
        assert_eq!(select(&idx, &f).len(), 1);
    }

    #[test]
    fn filters_compose() {
        let idx = index_from(vec![
            op("a", "POST", "/x", &["T"], false),
            op("b", "GET", "/x", &["T"], false),
            op("c", "POST", "/y", &["U"], false),
        ]);
        let f = ListFilter {
            tag: Some("T".to_string()),
            method: Some("POST".to_string()),
            ..Default::default()
        };
        let got = select(&idx, &f);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].operation_id, "a");
    }

    #[test]
    fn table_renders_header_and_count() {
        let idx = index_from(vec![op("a", "GET", "/a", &["X"], false)]);
        let (body, n) = render(&idx, &ListFilter::default(), ListOutput::Table);
        assert_eq!(n, 1);
        assert!(body.contains("OP_ID"));
        assert!(body.contains("/a"));
        assert!(body.contains("1 operation"));
    }

    #[test]
    fn table_marks_sse() {
        let idx = index_from(vec![op("a", "POST", "/a", &["X"], true)]);
        let (body, _) = render(&idx, &ListFilter::default(), ListOutput::Table);
        assert!(body.contains("[SSE]"));
    }

    #[test]
    fn json_output_parses() {
        let idx = index_from(vec![op("a", "GET", "/a", &["X"], false)]);
        let (body, _) = render(&idx, &ListFilter::default(), ListOutput::Json);
        let v: serde_json::Value = serde_json::from_str(&body).expect("valid json");
        assert!(v.is_array());
        assert_eq!(v[0]["operation_id"], "a");
    }

    #[test]
    fn empty_match_message() {
        let idx = index_from(vec![op("a", "GET", "/a", &[], false)]);
        let f = ListFilter {
            tag: Some("nope".to_string()),
            ..Default::default()
        };
        let (body, n) = render(&idx, &f, ListOutput::Table);
        assert_eq!(n, 0);
        assert!(body.contains("No operations match"));
    }
}
