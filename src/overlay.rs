//! OpenAPI Overlay 1.1 preprocessing using RFC 9535 JSONPath.
//!
//! Extensions retain their existing project-specific merge rules and are applied
//! first. Overlay files and actions run in declaration order. `extends` is target
//! identity metadata, never a request to load another document. Each action
//! snapshots its target locations and copy source before mutation. Repeated
//! selections affect a location once; overlapping locations are processed from
//! deepest to shallowest, with descending array indices for removal. All valid
//! `1.1.<numeric patch>` versions share the Overlay 1.1 behavior.

use crate::{GeneratorError, merge_schema_extensions};
use serde_json::{Map, Value};
use serde_json_path::JsonPath;
use std::path::{Component, Path, PathBuf};

/// Build the effective document before validation, defaults, or analysis.
pub fn preprocess_spec(
    spec: Value,
    schema_extensions: &[PathBuf],
    overlays: &[PathBuf],
) -> Result<Value, GeneratorError> {
    let mut document = merge_schema_extensions(spec, schema_extensions)?;
    for path in overlays {
        let content = std::fs::read_to_string(path).map_err(|error| {
            overlay_error(&path.display().to_string(), None, &error.to_string())
        })?;
        let overlay =
            crate::spec_source::parse_spec(&content, &path.to_string_lossy()).map_err(|error| {
                overlay_error(&path.display().to_string(), None, &error.to_string())
            })?;
        document = apply_overlay(document, &overlay, &path.display().to_string())?;
    }
    Ok(document)
}

/// Apply one JSON or YAML-decoded Overlay document in memory.
///
/// Diagnostics include `source` and the one-based action number. Removing the
/// document root is an error because it is not contained in a map or array.
/// An action specifying both update and copy is rejected: the standard makes
/// each inactive when the other is present. `remove: true` takes precedence.
pub fn apply_overlay(
    mut document: Value,
    overlay: &Value,
    source: &str,
) -> Result<Value, GeneratorError> {
    let run = || -> Result<(), String> {
        let object = require_object(overlay, "overlay document")?;
        validate_fields(object, &["overlay", "info", "extends", "actions"])?;
        let version = require_string(object, "overlay")?;
        let version_parts: Vec<_> = version.split('.').collect();
        if !matches!(version_parts.as_slice(), ["1", "1", patch]
            if !patch.is_empty() && patch.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(format!(
                "unsupported Overlay version {version:?}; expected 1.1.<numeric patch>"
            ));
        }
        let info = require_object(object.get("info").ok_or("missing info")?, "info")?;
        validate_fields(info, &["title", "version", "description"])?;
        require_string(info, "title")?;
        require_string(info, "version")?;
        optional_string(info, "description")?;
        optional_string(object, "extends")?;
        let actions = object
            .get("actions")
            .and_then(Value::as_array)
            .ok_or("actions must be a nonempty array")?;
        if actions.is_empty() {
            return Err("actions must be a nonempty array".into());
        }
        Ok(())
    };
    run().map_err(|error| overlay_error(source, None, &error.to_string()))?;
    // Document shape was checked above.
    if let Some(actions) = overlay.get("actions").and_then(Value::as_array) {
        for (index, action) in actions.iter().enumerate() {
            apply_action(&mut document, action)
                .map_err(|error| overlay_error(source, Some(index + 1), &error))?;
        }
    }
    Ok(document)
}

fn overlay_error(source: &str, action: Option<usize>, message: &str) -> GeneratorError {
    let location = action.map_or_else(String::new, |index| format!(", action {index}"));
    GeneratorError::ValidationError(format!("Overlay '{source}'{location}: {message}"))
}

fn require_object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn require_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field} must be a string"))
}

fn optional_string(object: &Map<String, Value>, field: &str) -> Result<(), String> {
    if object.contains_key(field) {
        require_string(object, field)?;
    }
    Ok(())
}

fn validate_fields(object: &Map<String, Value>, fields: &[&str]) -> Result<(), String> {
    for field in object.keys() {
        if !fields.contains(&field.as_str()) && !field.starts_with("x-") {
            return Err(format!("unknown Overlay field {field:?}"));
        }
    }
    Ok(())
}

fn kind(value: &Value) -> u8 {
    match value {
        Value::Object(_) => 0,
        Value::Array(_) => 1,
        _ => 2,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LocationElement {
    Name(String),
    Index(usize),
}

#[derive(Clone)]
struct Location {
    pointer: String,
    elements: Vec<LocationElement>,
}

fn apply_action(document: &mut Value, action: &Value) -> Result<(), String> {
    let action = require_object(action, "action")?;
    validate_fields(
        action,
        &["target", "description", "update", "copy", "remove"],
    )?;
    optional_string(action, "description")?;
    optional_string(action, "copy")?;
    let remove = match action.get("remove") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("remove must be a boolean".into()),
    };
    let target = require_string(action, "target")?;
    let query =
        JsonPath::parse(target).map_err(|error| format!("invalid target {target:?}: {error}"))?;
    let update = action.get("update");
    let copy = action.get("copy").and_then(Value::as_str);
    // Validate the active action before evaluating targets. A valid unmatched
    // action is a no-op, but an unmatched target cannot hide malformed syntax.
    let copy_query = if remove {
        None
    } else {
        if update.is_some() && copy.is_some() {
            return Err(
                "update and copy cannot both be active in one action; use separate ordered actions"
                    .into(),
            );
        }
        copy.map(|expression| {
            JsonPath::parse(expression)
                .map_err(|error| format!("invalid copy {expression:?}: {error}"))
        })
        .transpose()?
    };
    let selected = query.query_located(document).dedup();
    let mut locations: Vec<Location> = selected
        .locations()
        .map(|location| Location {
            pointer: location.to_json_pointer(),
            elements: location
                .iter()
                .map(|element| match element {
                    serde_json_path::PathElement::Name(name) => {
                        LocationElement::Name((*name).to_string())
                    }
                    serde_json_path::PathElement::Index(index) => LocationElement::Index(*index),
                })
                .collect(),
        })
        .collect();
    // A valid unmatched target is a successful no-op, including copy actions.
    // Copy cardinality is evaluated only when at least one target exists.
    if locations.is_empty() {
        return Ok(());
    }
    if !remove
        && (update.is_some() || copy.is_some())
        && selected
            .nodes()
            .map(kind)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1
    {
        return Err(
            "update/copy targets must be all objects, all arrays, or all primitives".into(),
        );
    }
    locations.sort_by(|left, right| {
        right
            .elements
            .len()
            .cmp(&left.elements.len())
            .then_with(|| right.elements.cmp(&left.elements))
    });
    if remove {
        if locations
            .iter()
            .any(|location| location.elements.is_empty())
        {
            return Err(
                "cannot remove the root document; it has no containing map or array".into(),
            );
        }
        for location in locations {
            remove_location(document, &location)?;
        }
        return Ok(());
    }
    let payload = if let Some(update) = update {
        update.clone()
    } else if let (Some(copy), Some(path)) = (copy, copy_query) {
        path.query(document)
            .exactly_one()
            .map_err(|error| format!("copy {copy:?} must select exactly one node: {error}"))?
            .clone()
    } else {
        return Ok(());
    };
    for location in locations {
        let target = document
            .pointer_mut(&location.pointer)
            .ok_or_else(|| format!("target {} disappeared during action", location.pointer))?;
        merge_target(target, &payload, &location.pointer)?;
    }
    Ok(())
}

fn remove_location(document: &mut Value, location: &Location) -> Result<(), String> {
    let (parent_pointer, _) = location
        .pointer
        .rsplit_once('/')
        .ok_or("missing parent location")?;
    let parent = document
        .pointer_mut(parent_pointer)
        .ok_or("missing target parent")?;
    match (parent, location.elements.last()) {
        (Value::Object(object), Some(LocationElement::Name(name))) => {
            object.remove(name);
        }
        (Value::Array(array), Some(LocationElement::Index(index))) if *index < array.len() => {
            array.remove(*index);
        }
        _ => return Err(format!("cannot remove target {}", location.pointer)),
    }
    Ok(())
}

fn merge_target(target: &mut Value, update: &Value, pointer: &str) -> Result<(), String> {
    match (target, update) {
        (Value::Object(target), Value::Object(update)) => {
            for (name, value) in update {
                if let Some(existing) = target.get_mut(name) {
                    let child = format!("{pointer}/{}", name.replace('~', "~0").replace('/', "~1"));
                    // Recursive object properties require matching value kinds.
                    if kind(existing) != kind(value) {
                        return Err(format!("incompatible value kinds at {child}"));
                    }
                    merge_target(existing, value, &child)?;
                } else {
                    target.insert(name.clone(), value.clone());
                }
            }
        }
        (Value::Array(target), Value::Array(update)) => target.extend(update.iter().cloned()),
        (Value::Array(target), value) => target.push(value.clone()),
        (target, update) if kind(target) == 2 && kind(update) == 2 => *target = update.clone(),
        _ => return Err(format!("incompatible value kinds at {pointer}")),
    }
    Ok(())
}

/// Validate an artifact filename relative to the output directory.
pub fn validate_artifact_path(path: &Path) -> Result<(), GeneratorError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(GeneratorError::ValidationError(format!(
            "artifact path '{}' must be a nonempty relative filename inside output_dir",
            path.display()
        )));
    }
    Ok(())
}

/// Deterministic JSON materialization of the effective document.
pub fn effective_spec_json(document: &Value) -> Result<String, GeneratorError> {
    // Rebuild maps recursively to preserve determinism even if a downstream
    // consumer enables serde_json's preserve_order feature through unification.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), canonical(value)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(array) => Value::Array(array.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&canonical(document))?
    ))
}

#[cfg(test)]
mod tests {
    use super::apply_overlay;
    use serde_json::json;

    #[test]
    fn inactive_actions_allow_mixed_target_kinds() {
        let document = json!([{}, [], null, 1, true, "text"]);
        let overlay = json!({"overlay": "1.1.0", "info": {"title": "No-op", "version": "1"}, "actions": [
            {"target": "$[*]"},
            {"target": "$[*]", "remove": false}
        ]});
        match apply_overlay(document.clone(), &overlay, "inactive.overlay.json") {
            Ok(effective) => assert_eq!(effective, document),
            Err(error) => panic!("inactive action failed: {error}"),
        }
    }

    #[test]
    fn overlay_one_one_patch_versions_are_ignored() {
        for version in ["1.1.0", "1.1.1", "1.1.98765432109876543210"] {
            let overlay = json!({"overlay": version, "info": {"title": "Version", "version": "1"}, "actions": [{"target": "$"}]});
            assert!(
                apply_overlay(json!({}), &overlay, "version.overlay.json").is_ok(),
                "{version}"
            );
        }
        for version in [
            "1.0.0", "1.2.0", "2.1.0", "1.1", "1.1.", "1.1.a", "1.1.-1", "1.1.0.1", " 1.1.0",
            "1.1.0 ",
        ] {
            let overlay = json!({"overlay": version, "info": {"title": "Version", "version": "1"}, "actions": [{"target": "$"}]});
            assert!(
                apply_overlay(json!({}), &overlay, "version.overlay.json").is_err(),
                "{version}"
            );
        }
    }

    #[test]
    fn unmatched_targets_do_not_hide_invalid_active_copy_actions() {
        for (action, expected) in [
            (
                json!({"target": "$.missing", "copy": "not-jsonpath"}),
                "invalid copy",
            ),
            (
                json!({"target": "$.missing", "update": null, "copy": "$.source"}),
                "update and copy",
            ),
        ] {
            let overlay = json!({"overlay": "1.1.0", "info": {"title": "Validation", "version": "1"}, "actions": [action]});
            let error = match apply_overlay(json!({}), &overlay, "invalid.overlay.json") {
                Err(error) => error.to_string(),
                Ok(_) => panic!("invalid action succeeded"),
            };
            assert!(error.contains(expected), "{error}");
            assert!(error.contains("invalid.overlay.json', action 1"), "{error}");
        }
    }

    #[test]
    fn remove_precedence_and_valid_unmatched_copy_cardinality_are_preserved() {
        let overlay = json!({"overlay": "1.1.0", "info": {"title": "No-op", "version": "1"}, "actions": [
            {"target": "$.missing", "copy": "$.missing_source"},
            {"target": "$.missing", "copy": "$[*]"},
            {"target": "$.remove", "remove": true, "update": {}, "copy": "not-jsonpath"},
            {"target": "$.missing", "remove": true, "update": {}, "copy": "not-jsonpath"}
        ]});
        match apply_overlay(
            json!({"remove": 1, "keep": 2}),
            &overlay,
            "precedence.overlay.json",
        ) {
            Ok(document) => assert_eq!(document, json!({"keep": 2})),
            Err(error) => panic!("valid no-op or remove action failed: {error}"),
        }
    }
}
