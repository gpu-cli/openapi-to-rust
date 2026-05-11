//! Formatting-preserving edits to the `[server].operations` array
//! in a project's `openapi-to-rust.toml`. Backs the
//! `server add` and `server remove` CLI subcommands.

use std::path::Path;
use toml_edit::{Array, DocumentMut, Item, Table, Value};

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("config is not valid TOML: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error("[server].operations exists but is not an array of strings — refusing to edit")]
    NotAnArray,
}

/// In-memory representation of a config we're editing. Created via
/// [`Editor::open`], mutated via [`Editor::add`] / [`Editor::remove`],
/// persisted via [`Editor::save`].
pub struct Editor {
    path: std::path::PathBuf,
    doc: DocumentMut,
}

impl Editor {
    pub fn open(path: &Path) -> Result<Self, EditError> {
        let text = std::fs::read_to_string(path).map_err(|source| EditError::Read {
            path: path.display().to_string(),
            source,
        })?;
        let doc: DocumentMut = text.parse()?;
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    /// Snapshot of the current `[server].operations` list (raw strings,
    /// not parsed selectors).
    pub fn operations(&self) -> Result<Vec<String>, EditError> {
        let Some(arr) = self.array_ref()? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(arr.len());
        for v in arr.iter() {
            match v.as_str() {
                Some(s) => out.push(s.to_string()),
                None => return Err(EditError::NotAnArray),
            }
        }
        Ok(out)
    }

    /// Append a selector to `[server].operations`, creating the
    /// section, `framework` key, and array as needed.
    /// Returns `true` if the entry was added, `false` if it was already
    /// present (no edit applied).
    pub fn add(&mut self, selector: &str) -> Result<bool, EditError> {
        if self.operations()?.iter().any(|s| s == selector) {
            return Ok(false);
        }
        let arr = self.ensure_array()?;
        arr.push(selector);
        Ok(true)
    }

    /// Remove the first entry equal to `selector`. Returns `true` if
    /// something was removed, `false` if it was absent.
    pub fn remove(&mut self, selector: &str) -> Result<bool, EditError> {
        let Some(arr) = self.array_mut()? else {
            return Ok(false);
        };
        let pos = arr.iter().position(|v| v.as_str() == Some(selector));
        match pos {
            Some(i) => {
                arr.remove(i);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn save(&self) -> Result<(), EditError> {
        std::fs::write(&self.path, self.doc.to_string()).map_err(|source| EditError::Write {
            path: self.path.display().to_string(),
            source,
        })
    }

    /// Render the current document as a string without writing. Useful
    /// for `--dry-run` and for tests.
    pub fn rendered(&self) -> String {
        self.doc.to_string()
    }

    // --- internals ---

    fn array_ref(&self) -> Result<Option<&Array>, EditError> {
        let Some(server) = self.doc.get("server").and_then(Item::as_table) else {
            return Ok(None);
        };
        let Some(item) = server.get("operations") else {
            return Ok(None);
        };
        match item.as_array() {
            Some(arr) => Ok(Some(arr)),
            None => Err(EditError::NotAnArray),
        }
    }

    fn array_mut(&mut self) -> Result<Option<&mut Array>, EditError> {
        let Some(server) = self.doc.get_mut("server").and_then(Item::as_table_mut) else {
            return Ok(None);
        };
        let Some(item) = server.get_mut("operations") else {
            return Ok(None);
        };
        match item.as_array_mut() {
            Some(arr) => Ok(Some(arr)),
            None => Err(EditError::NotAnArray),
        }
    }

    fn ensure_array(&mut self) -> Result<&mut Array, EditError> {
        // Ensure [server] table exists.
        if !matches!(self.doc.get("server"), Some(Item::Table(_))) {
            let mut tbl = Table::new();
            tbl.set_implicit(false);
            tbl.insert("framework", Item::Value(Value::from("axum")));
            tbl.insert("operations", Item::Value(Value::Array(Array::new())));
            self.doc.insert("server", Item::Table(tbl));
        }
        let server = self
            .doc
            .get_mut("server")
            .and_then(Item::as_table_mut)
            .ok_or(EditError::NotAnArray)?;
        if !server.contains_key("framework") {
            server.insert("framework", Item::Value(Value::from("axum")));
        }
        if !server.contains_key("operations") {
            server.insert("operations", Item::Value(Value::Array(Array::new())));
        }
        server
            .get_mut("operations")
            .and_then(Item::as_array_mut)
            .ok_or(EditError::NotAnArray)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn editor_from(contents: &str) -> (Editor, NamedTempFile) {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{contents}").unwrap();
        let ed = Editor::open(f.path()).unwrap();
        (ed, f)
    }

    #[test]
    fn add_creates_section_when_absent() {
        let (mut ed, _f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "src/gen"
module_name = "api"
"#,
        );
        assert!(ed.add("createResponse").unwrap());
        let rendered = ed.rendered();
        assert!(rendered.contains("[server]"));
        assert!(rendered.contains("framework = \"axum\""));
        assert!(rendered.contains("createResponse"));
    }

    #[test]
    fn add_preserves_existing_comments_and_keys() {
        let (mut ed, _f) = editor_from(
            r#"# Top-level comment
[generator]
spec_path = "x.json"  # path comment
output_dir = "src/gen"
module_name = "api"

[server]
framework = "axum"
# pinned for the chat replica service
operations = ["existing"]
"#,
        );
        assert!(ed.add("createResponse").unwrap());
        let r = ed.rendered();
        assert!(r.contains("# Top-level comment"));
        assert!(r.contains("# path comment"));
        assert!(r.contains("# pinned for the chat replica service"));
        assert!(r.contains("\"existing\""));
        assert!(r.contains("\"createResponse\""));
    }

    #[test]
    fn add_is_idempotent() {
        let (mut ed, _f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "g"
module_name = "m"

[server]
framework = "axum"
operations = ["createResponse"]
"#,
        );
        assert!(!ed.add("createResponse").unwrap());
        assert_eq!(ed.operations().unwrap(), vec!["createResponse".to_string()]);
    }

    #[test]
    fn remove_removes_first_match() {
        let (mut ed, _f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "g"
module_name = "m"

[server]
framework = "axum"
operations = ["a", "b", "c"]
"#,
        );
        assert!(ed.remove("b").unwrap());
        assert_eq!(
            ed.operations().unwrap(),
            vec!["a".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn remove_absent_is_noop_returns_false() {
        let (mut ed, _f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "g"
module_name = "m"

[server]
framework = "axum"
operations = ["a"]
"#,
        );
        assert!(!ed.remove("zzz").unwrap());
        assert_eq!(ed.operations().unwrap(), vec!["a".to_string()]);
    }

    #[test]
    fn remove_when_no_server_section_is_noop() {
        let (mut ed, _f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "g"
module_name = "m"
"#,
        );
        assert!(!ed.remove("a").unwrap());
    }

    #[test]
    fn rejects_non_string_array() {
        let (ed, _f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "g"
module_name = "m"

[server]
framework = "axum"
operations = [42]
"#,
        );
        assert!(matches!(ed.operations(), Err(EditError::NotAnArray)));
    }

    #[test]
    fn save_writes_to_disk() {
        let (mut ed, f) = editor_from(
            r#"[generator]
spec_path = "x.json"
output_dir = "g"
module_name = "m"
"#,
        );
        ed.add("foo").unwrap();
        ed.save().unwrap();
        let on_disk = std::fs::read_to_string(f.path()).unwrap();
        assert!(on_disk.contains("[server]"));
        assert!(on_disk.contains("\"foo\""));
    }
}
