use colored::Colorize;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use tracing::warn;

mod validator;

pub use validator::validate_metadata;

#[derive(Clone, Debug)]
pub enum ValidationError {
    MissingField(String),
    TypeMismatch {
        field: String,
        expected: String,
        actual: String,
    },
    ConstraintViolation {
        field: String,
        message: String,
    },
    RuleConditionFailed {
        message: String,
    },
}

impl ValidationError {
    pub fn with_field(&mut self, field: String) -> &Self {
        match self {
            Self::TypeMismatch { field: f, .. } => *f = field,
            Self::ConstraintViolation { field: f, .. } => *f = field,
            _ => {}
        }
        self
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "{} '{}'", "Missing field".bold(), field.bold()),
            Self::TypeMismatch {
                field,
                expected,
                actual,
            } => write!(
                f,
                "{} '{}': expected {}, got {}",
                "Type mismatch for field".bold(),
                field.bold(),
                expected.bold(),
                actual.bold()
            ),
            Self::ConstraintViolation { field, message } => {
                write!(
                    f,
                    "{} '{}': {}",
                    "Constraint violation for field".bold(),
                    field.bold(),
                    message
                )
            }
            Self::RuleConditionFailed { message } => {
                write!(f, "{}: {}", "Rule condition failed".bold(), message)
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentSchema {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub fields: HashMap<String, FieldDefinition>,
    #[serde(default, rename = "rules")] // Fix: handle TOML array format
    pub rules: Vec<ValidationRule>,
    #[serde(default, rename = "paths")]
    pub paths: HashMap<String, Box<ContentSchema>>,
}

// Struct to hold merged validation requirements
#[derive(Default, Debug)]
pub struct MergedSchema {
    pub required: Vec<String>,
    pub fields: HashMap<String, FieldDefinition>,
    pub rules: Vec<ValidationRule>,
}

impl ContentSchema {
    /// Resolves schema hierarchy for a content path
    pub fn resolve_path<'a>(&'a self, content_path: &str) -> Vec<&'a ContentSchema> {
        let mut nodes = vec![self];
        let mut current = self;

        // Split path into components (e.g. "posts/2023" -> ["posts", "2023"])
        for component in content_path.split('/').filter(|s| !s.is_empty()) {
            if let Some(child) = current.paths.get(component) {
                nodes.push(child);
                current = child;
            }
        }

        nodes
    }

    /// Merges schema hierarchy into final validation rules
    pub fn merge_hierarchy(nodes: &[&Self]) -> MergedSchema {
        // Only merge the hierarchy nodes in order (global -> specific)
        nodes.iter().fold(MergedSchema::default(), |mut acc, node| {
            // Merge required fields with deduplication
            let current_required = acc.required.clone();

            acc.required.extend(
                node.required
                    .iter()
                    .filter(|f| !current_required.contains(f))
                    .cloned(),
            );

            // Merge fields with later nodes overriding earlier ones
            for (k, v) in &node.fields {
                acc.fields.insert(k.clone(), v.clone());
            }

            // Merge rules while maintaining order
            acc.rules.extend(node.rules.iter().cloned());

            acc
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldDefinition {
    String {
        max_length: Option<usize>,
        pattern: Option<String>, // Regex patterns
    },
    Array {
        items: Box<FieldDefinition>,
        min_items: Option<usize>,
        max_items: Option<usize>,
        must_contain: Option<Vec<toml::Value>>,
    },
    Boolean,
    Object {
        schema: HashMap<String, FieldDefinition>,
    },
}

impl FieldDefinition {
    pub fn validate(&self, value: &toml::Value) -> Result<(), ValidationError> {
        match (self, value) {
            (
                FieldDefinition::String {
                    max_length,
                    pattern,
                },
                toml::Value::String(s),
            ) => {
                if let Some(max) = max_length {
                    if s.len() > *max {
                        return Err(ValidationError::ConstraintViolation {
                            field: String::new(),
                            message: format!("Exceeds max length {}", max),
                        });
                    }
                }
                if let Some(pattern) = pattern {
                    let re = match Regex::new(pattern) {
                        Ok(r) => r,
                        Err(_) => {
                            return Err(ValidationError::ConstraintViolation {
                                field: "pattern".into(),
                                message: format!("Invalid regex pattern: {}", pattern),
                            })
                        }
                    };
                    if !re.is_match(s) {
                        return Err(ValidationError::ConstraintViolation {
                            field: String::new(),
                            message: format!("No pattern matching {}", pattern),
                        });
                    }
                }
                Ok(())
            }
            (
                FieldDefinition::Array {
                    items: _,
                    min_items,
                    max_items,
                    must_contain,
                },
                toml::Value::Array(arr),
            ) => {
                if let Some(required_values) = must_contain {
                    for required in required_values {
                        if !arr.contains(required) {
                            return Err(ValidationError::ConstraintViolation {
                                field: String::new(),
                                message: format!("Missing value {}", required),
                            });
                        }
                    }
                }
                if let Some(min) = min_items {
                    if arr.len() < *min {
                        return Err(ValidationError::ConstraintViolation {
                            field: String::new(),
                            message: format!("Must contain at least {} value(s)", *min),
                        });
                    }
                }
                if let Some(max) = max_items {
                    if arr.len() > *max {
                        return Err(ValidationError::ConstraintViolation {
                            field: String::new(),
                            message: format!("Exceeds values limit (expected {} value(s))", *max),
                        });
                    }
                }
                Ok(())
            }
            (FieldDefinition::Boolean, value) => {
                if !value.is_bool() {
                    return Err(ValidationError::TypeMismatch {
                        field: String::new(),
                        expected: self.type_name(),
                        actual: value.to_string(),
                    });
                }
                Ok(())
            }
            _ => Err(ValidationError::TypeMismatch {
                field: String::new(), // Should populate field name from context
                expected: self.type_name(),
                actual: value.to_string(),
            }),
        }
    }

    fn type_name(&self) -> String {
        match self {
            FieldDefinition::String { .. } => "string",
            FieldDefinition::Array { .. } => "array",
            FieldDefinition::Boolean => "boolean",
            FieldDefinition::Object { .. } => "object",
        }
        .to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationRule {
    #[serde(rename = "if")]
    pub condition: HashMap<String, toml::Value>,
    pub then: RuleAction,
}

impl ValidationRule {
    pub fn applies(
        &self,
        metadata: &HashMap<String, toml::Value>,
    ) -> Result<bool, ValidationError> {
        self.condition
            .iter()
            .try_fold(true, |acc, (field, expected)| match metadata.get(field) {
                Some(actual) => {
                    if actual.type_str() != expected.type_str() {
                        Err(ValidationError::RuleConditionFailed {
                            message: format!(
                                "Type mismatch in condition field '{}': expected {}, got {}",
                                field,
                                expected.type_str(),
                                actual.type_str()
                            ),
                        })
                    } else {
                        Ok(acc && actual == expected)
                    }
                }
                None => {
                    warn!("Missing condition field '{}'", field);
                    Ok(false)
                }
            })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleAction {
    pub required: Option<Vec<String>>,
    pub fields: Option<HashMap<String, FieldDefinition>>,
}

pub fn format_errors(
    file_path: &Path,
    schema_path: &str,
    errors: &[ValidationError],
    as_warnings: bool,
) -> String {
    let mut output = format!(
        "{} '{}'\n",
        format!(
            "Validation {} for",
            if as_warnings { "issues" } else { "failed" }
        )
        .bold(),
        file_path.display()
    );
    output.push_str(&format!(
        "  {} {}: '{}'\n",
        "→".blue(),
        "Schema applied".bold(),
        schema_path
    ));
    for error in errors {
        output.push_str(&format!("  {} {}\n", "→".blue(), error));
    }
    output
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn str_array(items: &[&str]) -> toml::Value {
        toml::Value::Array(
            items
                .iter()
                .map(|s| toml::Value::String(s.to_string()))
                .collect(),
        )
    }

    fn bare_schema(required: &[&str]) -> ContentSchema {
        ContentSchema {
            required: required.iter().map(|s| s.to_string()).collect(),
            fields: HashMap::new(),
            rules: Vec::new(),
            paths: HashMap::new(),
        }
    }

    // FieldDefinition::String

    #[test]
    fn string_valid() {
        let def = FieldDefinition::String {
            max_length: None,
            pattern: None,
        };
        assert!(def.validate(&toml::Value::String("hello".into())).is_ok());
    }

    #[test]
    fn string_within_max_length_ok() {
        let def = FieldDefinition::String {
            max_length: Some(10),
            pattern: None,
        };
        assert!(def.validate(&toml::Value::String("hello".into())).is_ok());
    }

    #[test]
    fn string_at_exact_max_length_ok() {
        let def = FieldDefinition::String {
            max_length: Some(5),
            pattern: None,
        };
        assert!(def.validate(&toml::Value::String("hello".into())).is_ok());
    }

    #[test]
    fn string_exceeds_max_length() {
        let def = FieldDefinition::String {
            max_length: Some(3),
            pattern: None,
        };
        let err = def
            .validate(&toml::Value::String("hello".into()))
            .unwrap_err();
        assert!(matches!(err, ValidationError::ConstraintViolation { .. }));
    }

    #[test]
    fn string_matching_pattern_ok() {
        let def = FieldDefinition::String {
            max_length: None,
            pattern: Some(r"^\d+$".into()),
        };
        assert!(def.validate(&toml::Value::String("1234".into())).is_ok());
    }

    #[test]
    fn string_non_matching_pattern_errors() {
        let def = FieldDefinition::String {
            max_length: None,
            pattern: Some(r"^\d+$".into()),
        };
        let err = def
            .validate(&toml::Value::String("abc".into()))
            .unwrap_err();
        assert!(matches!(err, ValidationError::ConstraintViolation { .. }));
    }

    #[test]
    fn string_wrong_type_errors() {
        let def = FieldDefinition::String {
            max_length: None,
            pattern: None,
        };
        let err = def.validate(&toml::Value::Boolean(true)).unwrap_err();
        assert!(matches!(err, ValidationError::TypeMismatch { .. }));
    }

    // FieldDefinition::Boolean

    #[test]
    fn boolean_true_valid() {
        assert!(FieldDefinition::Boolean
            .validate(&toml::Value::Boolean(true))
            .is_ok());
    }

    #[test]
    fn boolean_false_valid() {
        assert!(FieldDefinition::Boolean
            .validate(&toml::Value::Boolean(false))
            .is_ok());
    }

    #[test]
    fn boolean_string_errors() {
        let err = FieldDefinition::Boolean
            .validate(&toml::Value::String("true".into()))
            .unwrap_err();
        assert!(matches!(err, ValidationError::TypeMismatch { .. }));
    }

    // FieldDefinition::Array

    #[test]
    fn array_valid_no_constraints() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::String {
                max_length: None,
                pattern: None,
            }),
            min_items: None,
            max_items: None,
            must_contain: None,
        };
        assert!(def.validate(&str_array(&["a", "b"])).is_ok());
    }

    #[test]
    fn array_min_items_exactly_satisfied() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::Boolean),
            min_items: Some(2),
            max_items: None,
            must_contain: None,
        };
        assert!(def.validate(&str_array(&["a", "b"])).is_ok());
    }

    #[test]
    fn array_min_items_violated() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::Boolean),
            min_items: Some(3),
            max_items: None,
            must_contain: None,
        };
        let err = def.validate(&str_array(&["a", "b"])).unwrap_err();
        assert!(matches!(err, ValidationError::ConstraintViolation { .. }));
    }

    #[test]
    fn array_max_items_exactly_satisfied() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::Boolean),
            min_items: None,
            max_items: Some(3),
            must_contain: None,
        };
        assert!(def.validate(&str_array(&["a", "b"])).is_ok());
    }

    #[test]
    fn array_max_items_violated() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::Boolean),
            min_items: None,
            max_items: Some(2),
            must_contain: None,
        };
        let err = def.validate(&str_array(&["a", "b", "c"])).unwrap_err();
        assert!(matches!(err, ValidationError::ConstraintViolation { .. }));
    }

    #[test]
    fn array_must_contain_present_ok() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::String {
                max_length: None,
                pattern: None,
            }),
            min_items: None,
            max_items: None,
            must_contain: Some(vec![toml::Value::String("norgolith".into())]),
        };
        assert!(def.validate(&str_array(&["foo", "norgolith"])).is_ok());
    }

    #[test]
    fn array_must_contain_absent_errors() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::String {
                max_length: None,
                pattern: None,
            }),
            min_items: None,
            max_items: None,
            must_contain: Some(vec![toml::Value::String("norgolith".into())]),
        };
        let err = def.validate(&str_array(&["foo", "bar"])).unwrap_err();
        assert!(matches!(err, ValidationError::ConstraintViolation { .. }));
    }

    #[test]
    fn array_wrong_type_errors() {
        let def = FieldDefinition::Array {
            items: Box::new(FieldDefinition::Boolean),
            min_items: None,
            max_items: None,
            must_contain: None,
        };
        let err = def
            .validate(&toml::Value::String("not an array".into()))
            .unwrap_err();
        assert!(matches!(err, ValidationError::TypeMismatch { .. }));
    }

    // ContentSchema::resolve_path

    #[test]
    fn resolve_path_root_only() {
        let schema = bare_schema(&["title"]);
        assert_eq!(schema.resolve_path("about").len(), 1);
    }

    #[test]
    fn resolve_path_single_child() {
        let mut schema = bare_schema(&["title"]);
        schema
            .paths
            .insert("posts".into(), Box::new(bare_schema(&["category"])));
        let nodes = schema.resolve_path("posts/my-post");
        assert_eq!(nodes.len(), 2);
        assert!(nodes[1].required.contains(&"category".to_string()));
    }

    #[test]
    fn resolve_path_unknown_component_stays_at_root() {
        let schema = bare_schema(&["title"]);
        assert_eq!(schema.resolve_path("nonexistent/deep").len(), 1);
    }

    #[test]
    fn resolve_path_partial_match_stops_at_last_known() {
        let mut schema = bare_schema(&["title"]);
        schema
            .paths
            .insert("posts".into(), Box::new(bare_schema(&["category"])));
        // "posts" matches, "2025" has no child entry under posts
        let nodes = schema.resolve_path("posts/2025/my-post");
        assert_eq!(nodes.len(), 2);
    }

    // ContentSchema::merge_hierarchy

    #[test]
    fn merge_single_node_identity() {
        let schema = bare_schema(&["title", "author"]);
        let merged = ContentSchema::merge_hierarchy(&[&schema]);
        assert_eq!(merged.required.len(), 2);
    }

    #[test]
    fn merge_deduplicates_required_fields() {
        let a = bare_schema(&["title", "author"]);
        let b = bare_schema(&["author", "created"]);
        let merged = ContentSchema::merge_hierarchy(&[&a, &b]);
        assert_eq!(merged.required.iter().filter(|f| *f == "author").count(), 1);
        assert_eq!(merged.required.len(), 3);
    }

    #[test]
    fn merge_later_field_definition_overrides_earlier() {
        let mut a = bare_schema(&[]);
        a.fields.insert(
            "title".into(),
            FieldDefinition::String {
                max_length: Some(50),
                pattern: None,
            },
        );
        let mut b = bare_schema(&[]);
        b.fields.insert(
            "title".into(),
            FieldDefinition::String {
                max_length: Some(120),
                pattern: None,
            },
        );
        let merged = ContentSchema::merge_hierarchy(&[&a, &b]);
        match merged.fields.get("title").unwrap() {
            FieldDefinition::String { max_length, .. } => assert_eq!(*max_length, Some(120)),
            _ => panic!("unexpected field definition type"),
        }
    }

    #[test]
    fn merge_rules_accumulate_in_order() {
        let mut a = bare_schema(&[]);
        a.rules.push(ValidationRule {
            condition: HashMap::from([("draft".into(), toml::Value::Boolean(false))]),
            then: RuleAction {
                required: Some(vec!["publish_date".into()]),
                fields: None,
            },
        });
        let mut b = bare_schema(&[]);
        b.rules.push(ValidationRule {
            condition: HashMap::from([("featured".into(), toml::Value::Boolean(true))]),
            then: RuleAction {
                required: Some(vec!["hero_image".into()]),
                fields: None,
            },
        });
        let merged = ContentSchema::merge_hierarchy(&[&a, &b]);
        assert_eq!(merged.rules.len(), 2);
    }

    // ValidationRule::applies

    fn draft_rule() -> ValidationRule {
        ValidationRule {
            condition: HashMap::from([("draft".into(), toml::Value::Boolean(false))]),
            then: RuleAction {
                required: Some(vec!["publish_date".into()]),
                fields: None,
            },
        }
    }

    #[test]
    fn rule_applies_when_condition_matches() {
        let meta = HashMap::from([("draft".into(), toml::Value::Boolean(false))]);
        assert!(matches!(draft_rule().applies(&meta), Ok(true)));
    }

    #[test]
    fn rule_does_not_apply_when_value_differs() {
        let meta = HashMap::from([("draft".into(), toml::Value::Boolean(true))]);
        assert!(matches!(draft_rule().applies(&meta), Ok(false)));
    }

    #[test]
    fn rule_skips_when_condition_field_missing() {
        assert!(matches!(draft_rule().applies(&HashMap::new()), Ok(false)));
    }

    #[test]
    fn rule_errors_on_type_mismatch_in_condition() {
        let meta = HashMap::from([("draft".into(), toml::Value::String("false".into()))]);
        assert!(matches!(
            draft_rule().applies(&meta),
            Err(ValidationError::RuleConditionFailed { .. })
        ));
    }
}
