// Shared by the planner and its emitted round-trip test. Compile only queried
// schema targets: build_map eagerly recompiles every JSON object with a fresh
// reference graph, which can retain many GiB for a large connected schema.
const MAX_CACHED_SCHEMA_VALIDATORS: usize = 32;
const SCHEMA_ORACLE_URI: &str = "https://openapi-to-rust.invalid/schema-roundtrip";
type CachedSchemaValidator = Option<std::rc::Rc<jsonschema::Validator>>;

struct SchemaValidators {
    registry: jsonschema::Registry<'static>,
    draft: jsonschema::Draft,
    cache: std::cell::RefCell<std::collections::VecDeque<(String, CachedSchemaValidator)>>,
}

impl SchemaValidators {
    fn new(
        document: &serde_json::Value,
        draft: jsonschema::Draft,
    ) -> Result<Self, jsonschema::ReferencingError> {
        let registry = jsonschema::Registry::new()
            .draft(draft)
            .add(SCHEMA_ORACLE_URI, document.clone())?
            .prepare()?;
        Ok(Self {
            registry,
            draft,
            cache: std::cell::RefCell::new(Default::default()),
        })
    }

    fn get(&self, pointer: &str) -> CachedSchemaValidator {
        let fragment = pointer.strip_prefix('#')?;
        if !fragment.is_empty() && !fragment.starts_with('/') {
            return None;
        }
        let mut cache = self.cache.borrow_mut();
        if let Some(index) = cache.iter().position(|(key, _)| key == pointer) {
            let entry = cache.remove(index)?;
            let validator = entry.1.clone();
            cache.push_front(entry);
            return validator;
        }
        let mut reference = format!("{SCHEMA_ORACLE_URI}#");
        for byte in fragment.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                reference.push(char::from(byte));
            } else {
                reference.push_str(&format!("%{byte:02X}"));
            }
        }
        let validator = jsonschema::options()
            .with_draft(self.draft)
            .should_validate_formats(true)
            .with_pattern_options(jsonschema::PatternOptions::regex())
            .with_registry(&self.registry)
            .build(&serde_json::json!({"$ref": reference}))
            .ok()
            .map(std::rc::Rc::new);
        cache.push_front((pointer.to_string(), validator.clone()));
        if cache.len() > MAX_CACHED_SCHEMA_VALIDATORS {
            cache.pop_back();
        }
        validator
    }
}
