//! Server operation selectors.
//!
//! Three forms (priority order in spec resolution):
//!   1. `operationId`        — bare identifier
//!   2. `METHOD /path`       — exact verb + path match
//!   3. `tag:<name>`         — expands to every op with that tag
//!
//! Parsing is spec-independent; resolution requires an [`OperationIndex`].

use super::{OperationIndex, OperationSummary};
use std::fmt;

/// One parsed selector. Construct via [`Selector::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    OperationId(String),
    MethodPath { method: String, path: String },
    Tag(String),
}

impl Selector {
    /// Parse a selector string. Whitespace is trimmed.
    ///
    /// Disambiguation:
    /// - Starts with `tag:` → [`Selector::Tag`].
    /// - Contains whitespace → split on first whitespace, treat as METHOD PATH.
    /// - Otherwise → operationId.
    pub fn parse(input: &str) -> Result<Self, SelectorParseError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(SelectorParseError::Empty);
        }

        if let Some(rest) = trimmed.strip_prefix("tag:") {
            let tag = rest.trim();
            if tag.is_empty() {
                return Err(SelectorParseError::EmptyTag);
            }
            return Ok(Self::Tag(tag.to_string()));
        }

        // METHOD PATH form: detect by leading whitespace-separated method
        // token followed by `/`-prefixed path.
        if let Some((method, path)) = split_method_path(trimmed) {
            return Ok(Self::MethodPath {
                method: method.to_ascii_uppercase(),
                path: path.to_string(),
            });
        }

        if trimmed.chars().any(char::is_whitespace) {
            return Err(SelectorParseError::WhitespaceInOpId(trimmed.to_string()));
        }

        Ok(Self::OperationId(trimmed.to_string()))
    }
}

fn split_method_path(s: &str) -> Option<(&str, &str)> {
    let (head, tail) = s.split_once(char::is_whitespace)?;
    let tail = tail.trim_start();
    if !tail.starts_with('/') || !is_http_method_token(head) {
        return None;
    }
    Some((head, tail))
}

/// RFC 9110 method names are case-sensitive `token` values. OpenAPI 3.2 adds
/// `QUERY` and permits arbitrary `additionalOperations`, so selector parsing
/// must not hard-code the legacy eight verbs.
fn is_http_method_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationId(id) => f.write_str(id),
            Self::MethodPath { method, path } => write!(f, "{method} {path}"),
            Self::Tag(t) => write!(f, "tag:{t}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectorParseError {
    #[error("selector is empty")]
    Empty,
    #[error("`tag:` selector has no tag name")]
    EmptyTag,
    #[error("selector `{0}` contains whitespace but is not a `METHOD /path` form")]
    WhitespaceInOpId(String),
}

/// Reason resolution failed, with a fuzzy-match suggestion when possible.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectorResolveError {
    #[error(
        "operationId `{requested}` is ambiguous after generator disambiguation: {matches}. Use a renamed ID shown above for renamed endpoints; select the unchanged `{requested}` endpoint with an exact `METHOD /path` selector"
    )]
    AmbiguousOperationId { requested: String, matches: String },
    #[error(
        "operationId `{requested}` was renamed to `{generated}` because its Rust name collides with another operation (`{method} {path}`). Use `{generated}` or the exact `METHOD /path` selector"
    )]
    RenamedOperationId {
        requested: String,
        generated: String,
        method: String,
        path: String,
    },
    #[error(
        "no operation with id `{requested}`{}",
        format_suggestion(.suggestion.as_deref())
    )]
    UnknownOperationId {
        requested: String,
        suggestion: Option<String>,
    },
    #[error(
        "no operation at `{method} {path}`{}",
        format_suggestion(.suggestion.as_deref())
    )]
    UnknownMethodPath {
        method: String,
        path: String,
        suggestion: Option<String>,
    },
    #[error(
        "no operations tagged `{requested}`{}",
        format_suggestion(.suggestion.as_deref())
    )]
    UnknownTag {
        requested: String,
        suggestion: Option<String>,
    },
}

fn format_suggestion(s: Option<&str>) -> String {
    match s {
        Some(s) => format!(". Did you mean `{s}`?"),
        None => String::new(),
    }
}

/// Outcome of resolving a list of selectors against an index.
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    /// Resolved operations in input order. Duplicates collapsed by operationId.
    pub operations: Vec<OperationSummary>,
    /// Duplicate selectors that resolved to an already-included operation.
    pub duplicates: Vec<String>,
}

/// Resolve a list of parsed selectors against an [`OperationIndex`].
/// Returns either every operation matched (in input order, deduped) or
/// the first error encountered (fail-fast — codegen requires every
/// selector to bind so unbound entries don't silently get dropped).
pub fn resolve(
    selectors: &[Selector],
    index: &OperationIndex,
) -> Result<Resolution, SelectorResolveError> {
    let mut out: Vec<OperationSummary> = Vec::new();
    let mut duplicates: Vec<String> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for sel in selectors {
        match resolve_one(sel, index)? {
            ResolvedOne::Single(op) => {
                if seen_ids.insert(op.operation_id.clone()) {
                    out.push(op);
                } else {
                    duplicates.push(sel.to_string());
                }
            }
            ResolvedOne::Many(ops) => {
                for op in ops {
                    if seen_ids.insert(op.operation_id.clone()) {
                        out.push(op);
                    } else {
                        duplicates.push(format!("{sel} → {}", op.operation_id));
                    }
                }
            }
        }
    }

    Ok(Resolution {
        operations: out,
        duplicates,
    })
}

enum ResolvedOne {
    Single(OperationSummary),
    Many(Vec<OperationSummary>),
}

fn resolve_one(
    sel: &Selector,
    index: &OperationIndex,
) -> Result<ResolvedOne, SelectorResolveError> {
    match sel {
        Selector::OperationId(id) => {
            if let Some(aliases) = index.operation_id_aliases(id) {
                if aliases.len() > 1 {
                    let matches = aliases
                        .iter()
                        .filter_map(|generated| {
                            index
                                .operations()
                                .iter()
                                .find(|op| &op.operation_id == generated)
                                .map(|op| {
                                    format!("`{} {}` → `{}`", op.method, op.path, op.operation_id)
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(SelectorResolveError::AmbiguousOperationId {
                        requested: id.clone(),
                        matches,
                    });
                }

                // A disambiguated generated ID can itself equal another
                // operation's raw ID. Keep emitted IDs directly selectable;
                // the raw-ID rename diagnostic only applies when there is no
                // exact generated operation with this ID.
                if let Some(op) = index.operations().iter().find(|op| &op.operation_id == id) {
                    return Ok(ResolvedOne::Single(op.clone()));
                }
                if let Some(generated) = aliases.first()
                    && generated != id
                    && let Some(op) = index
                        .operations()
                        .iter()
                        .find(|op| &op.operation_id == generated)
                {
                    return Err(SelectorResolveError::RenamedOperationId {
                        requested: id.clone(),
                        generated: generated.clone(),
                        method: op.method.clone(),
                        path: op.path.clone(),
                    });
                }
            }

            index
                .operations()
                .iter()
                .find(|op| &op.operation_id == id)
                .cloned()
                .map(ResolvedOne::Single)
                .ok_or_else(|| SelectorResolveError::UnknownOperationId {
                    requested: id.clone(),
                    suggestion: suggest_op_id(id, index),
                })
        }
        Selector::MethodPath { method, path } => index
            .operations()
            .iter()
            .find(|op| &op.method == method && &op.path == path)
            .cloned()
            .map(ResolvedOne::Single)
            .ok_or_else(|| SelectorResolveError::UnknownMethodPath {
                method: method.clone(),
                path: path.clone(),
                suggestion: suggest_method_path(method, path, index),
            }),
        Selector::Tag(tag) => {
            let matched: Vec<OperationSummary> = index
                .operations()
                .iter()
                .filter(|op| op.tags.iter().any(|t| t == tag))
                .cloned()
                .collect();
            if matched.is_empty() {
                Err(SelectorResolveError::UnknownTag {
                    requested: tag.clone(),
                    suggestion: suggest_tag(tag, index),
                })
            } else {
                Ok(ResolvedOne::Many(matched))
            }
        }
    }
}

/// Levenshtein distance between two short strings. Implementation
/// uses two rolling rows to stay allocation-light for the small
/// strings we compare (operationIds, paths, tags).
fn levenshtein(a: &str, b: &str) -> usize {
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Threshold scales with the longer string so long ids tolerate more typos.
fn suggest_threshold(needle: &str) -> usize {
    let len = needle.chars().count();
    if len <= 4 {
        1
    } else if len <= 8 {
        2
    } else {
        3
    }
}

fn suggest_op_id(needle: &str, index: &OperationIndex) -> Option<String> {
    let threshold = suggest_threshold(needle);
    index
        .operations()
        .iter()
        .filter(|op| !op.operation_id.is_empty())
        .map(|op| {
            (
                op.operation_id.clone(),
                levenshtein(needle, &op.operation_id),
            )
        })
        .filter(|(_, d)| *d <= threshold)
        .min_by_key(|(_, d)| *d)
        .map(|(id, _)| id)
}

fn suggest_method_path(method: &str, path: &str, index: &OperationIndex) -> Option<String> {
    let threshold = suggest_threshold(path);
    let candidate = index
        .operations()
        .iter()
        .filter(|op| op.method == method)
        .map(|op| (op.path.clone(), levenshtein(path, &op.path)))
        .filter(|(_, d)| *d <= threshold)
        .min_by_key(|(_, d)| *d);
    candidate.map(|(p, _)| format!("{method} {p}"))
}

fn suggest_tag(needle: &str, index: &OperationIndex) -> Option<String> {
    let threshold = suggest_threshold(needle);
    let mut best: Option<(String, usize)> = None;
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for op in index.operations() {
        for t in &op.tags {
            if !seen.insert(t.as_str()) {
                continue;
            }
            let d = levenshtein(needle, t);
            if d <= threshold && best.as_ref().is_none_or(|(_, bd)| d < *bd) {
                best = Some((t.clone(), d));
            }
        }
    }
    best.map(|(t, _)| format!("tag:{t}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::OperationSummary;

    fn op(id: &str, method: &str, path: &str, tags: &[&str]) -> OperationSummary {
        OperationSummary {
            operation_id: id.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            supports_streaming: false,
        }
    }

    fn idx(ops: Vec<OperationSummary>) -> OperationIndex {
        OperationIndex::from_summaries(ops)
    }

    // --- parser ---

    #[test]
    fn parse_op_id() {
        assert_eq!(
            Selector::parse("createResponse").unwrap(),
            Selector::OperationId("createResponse".into())
        );
    }

    #[test]
    fn parse_method_path_uppercases_verb() {
        assert_eq!(
            Selector::parse("post /v1/messages").unwrap(),
            Selector::MethodPath {
                method: "POST".into(),
                path: "/v1/messages".into()
            }
        );
    }

    #[test]
    fn parse_openapi_32_and_extension_methods() {
        assert_eq!(
            Selector::parse("query /v1/search").unwrap(),
            Selector::MethodPath {
                method: "QUERY".into(),
                path: "/v1/search".into(),
            }
        );
        assert_eq!(
            Selector::parse("purge /cache").unwrap(),
            Selector::MethodPath {
                method: "PURGE".into(),
                path: "/cache".into(),
            }
        );
    }

    #[test]
    fn parse_tag() {
        assert_eq!(
            Selector::parse("tag:Chat").unwrap(),
            Selector::Tag("Chat".into())
        );
    }

    #[test]
    fn parse_empty_errors() {
        assert!(matches!(
            Selector::parse("   "),
            Err(SelectorParseError::Empty)
        ));
        assert!(matches!(
            Selector::parse("tag:"),
            Err(SelectorParseError::EmptyTag)
        ));
    }

    #[test]
    fn parse_whitespace_in_op_id_errors() {
        // `foo bar` looks like METHOD PATH but `foo` isn't a verb and `bar`
        // doesn't start with `/`, so it's an invalid op-id form.
        assert!(matches!(
            Selector::parse("foo bar"),
            Err(SelectorParseError::WhitespaceInOpId(_))
        ));
    }

    #[test]
    fn parse_strips_whitespace() {
        assert_eq!(
            Selector::parse("  createResponse  ").unwrap(),
            Selector::OperationId("createResponse".into())
        );
    }

    // --- resolution ---

    #[test]
    fn resolve_op_id_hit() {
        let i = idx(vec![op(
            "createResponse",
            "POST",
            "/responses",
            &["Responses"],
        )]);
        let r = resolve(&[Selector::OperationId("createResponse".into())], &i).unwrap();
        assert_eq!(r.operations.len(), 1);
        assert_eq!(r.operations[0].operation_id, "createResponse");
    }

    #[test]
    fn resolve_op_id_miss_suggests() {
        let i = idx(vec![op("createResponse", "POST", "/responses", &[])]);
        let err = resolve(&[Selector::OperationId("createRespons".into())], &i).unwrap_err();
        match err {
            SelectorResolveError::UnknownOperationId { suggestion, .. } => {
                assert_eq!(suggestion.as_deref(), Some("createResponse"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn duplicate_raw_operation_id_is_actionably_ambiguous() {
        let mut aliases = std::collections::BTreeMap::new();
        aliases.insert(
            "foo".to_string(),
            vec!["foo".to_string(), "foo_post".to_string()],
        );
        let i = idx(vec![
            op("foo", "GET", "/first", &[]),
            op("foo_post", "POST", "/second", &[]),
        ])
        .with_aliases(aliases);

        let error = resolve(&[Selector::OperationId("foo".into())], &i).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("ambiguous"));
        assert!(message.contains("foo_post"));
        assert!(message.contains("METHOD /path"));
    }

    #[test]
    fn renamed_case_colliding_operation_id_reports_emitted_name() {
        let mut aliases = std::collections::BTreeMap::new();
        aliases.insert("Foo".to_string(), vec!["Foo_post".to_string()]);
        let i = idx(vec![
            op("foo", "GET", "/first", &[]),
            op("Foo_post", "POST", "/second", &[]),
        ])
        .with_aliases(aliases);

        let error = resolve(&[Selector::OperationId("Foo".into())], &i).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("renamed to `Foo_post`"));
        assert!(message.contains("POST /second"));

        let resolved = resolve(&[Selector::OperationId("Foo_post".into())], &i).unwrap();
        assert_eq!(resolved.operations[0].operation_id, "Foo_post");
    }

    #[test]
    fn emitted_id_wins_when_it_matches_another_operations_raw_id() {
        let mut aliases = std::collections::BTreeMap::new();
        aliases.insert(
            "foo".to_string(),
            vec!["foo".to_string(), "foo_post".to_string()],
        );
        aliases.insert("foo_post".to_string(), vec!["foo_post_get".to_string()]);
        let i = idx(vec![
            op("foo", "GET", "/first", &[]),
            op("foo_post", "POST", "/second", &[]),
            op("foo_post_get", "GET", "/third", &[]),
        ])
        .with_aliases(aliases);

        let resolved = resolve(&[Selector::OperationId("foo_post".into())], &i).unwrap();
        assert_eq!(resolved.operations[0].path, "/second");
    }

    #[test]
    fn resolve_method_path_hit() {
        let i = idx(vec![op("m", "POST", "/v1/messages", &[])]);
        let r = resolve(
            &[Selector::MethodPath {
                method: "POST".into(),
                path: "/v1/messages".into(),
            }],
            &i,
        )
        .unwrap();
        assert_eq!(r.operations.len(), 1);
    }

    #[test]
    fn resolve_method_path_miss_suggests_same_method_only() {
        let i = idx(vec![
            op("a", "POST", "/v1/messages", &[]),
            op("b", "GET", "/v1/messagex", &[]),
        ]);
        let err = resolve(
            &[Selector::MethodPath {
                method: "POST".into(),
                path: "/v1/messagex".into(),
            }],
            &i,
        )
        .unwrap_err();
        match err {
            SelectorResolveError::UnknownMethodPath { suggestion, .. } => {
                // Same-method nearest is /v1/messages (distance 1)
                assert_eq!(suggestion.as_deref(), Some("POST /v1/messages"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn resolve_tag_expands() {
        let i = idx(vec![
            op("a", "GET", "/a", &["Files"]),
            op("b", "POST", "/b", &["Files"]),
            op("c", "POST", "/c", &["Chat"]),
        ]);
        let r = resolve(&[Selector::Tag("Files".into())], &i).unwrap();
        assert_eq!(r.operations.len(), 2);
        assert_eq!(r.operations[0].operation_id, "a");
        assert_eq!(r.operations[1].operation_id, "b");
    }

    #[test]
    fn resolve_tag_miss_suggests() {
        let i = idx(vec![op("a", "GET", "/a", &["Embeddings"])]);
        let err = resolve(&[Selector::Tag("Embedding".into())], &i).unwrap_err();
        match err {
            SelectorResolveError::UnknownTag { suggestion, .. } => {
                assert_eq!(suggestion.as_deref(), Some("tag:Embeddings"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn resolve_dedup_within_run() {
        let i = idx(vec![op("a", "GET", "/a", &["T"])]);
        let r = resolve(
            &[Selector::OperationId("a".into()), Selector::Tag("T".into())],
            &i,
        )
        .unwrap();
        assert_eq!(r.operations.len(), 1);
        assert_eq!(r.duplicates.len(), 1);
        assert!(r.duplicates[0].contains("tag:T"));
    }

    #[test]
    fn resolve_preserves_input_order() {
        let i = idx(vec![
            op("a", "GET", "/a", &[]),
            op("b", "POST", "/b", &[]),
            op("c", "PUT", "/c", &[]),
        ]);
        let r = resolve(
            &[
                Selector::OperationId("c".into()),
                Selector::OperationId("a".into()),
                Selector::OperationId("b".into()),
            ],
            &i,
        )
        .unwrap();
        let ids: Vec<&str> = r
            .operations
            .iter()
            .map(|o| o.operation_id.as_str())
            .collect();
        assert_eq!(ids, ["c", "a", "b"]);
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}
