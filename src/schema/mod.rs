use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

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
            Self::MissingField(field) => write!(f, "Missing field '{}'", field),
            Self::TypeMismatch {
                field,
                expected,
                actual,
            } => write!(
                f,
                "Type mismatch for field '{}': expected {}, got {}",
                field, expected, actual
            ),
            Self::ConstraintViolation { field, message } => {
                write!(f, "Constraint violation for field '{}': {}", field, message)
            }
            Self::RuleConditionFailed { message } => {
                write!(f, "Rule condition failed: {}", message)
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
                    let re = Regex::new(pattern).unwrap();
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
                    if !arr.len() < *min {
                        return Err(ValidationError::ConstraintViolation {
                            field: String::new(),
                            message: format!("Must contain at least {} value(s)", *min),
                        });
                    }
                }
                if let Some(max) = max_items {
                    if !arr.len() < *max {
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
                None => Err(ValidationError::RuleConditionFailed {
                    message: format!("Missing condition field '{}'", field),
                }),
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
        "{}: Validation {} for '{}'\n",
        if as_warnings { "Warning" } else { "Error" },
        if as_warnings { "issues" } else { "failed" },
        file_path.display()
    );
    output.push_str(&format!("  → Schema applied: '{}'\n", schema_path));
    for error in errors {
        output.push_str(&format!("  → {}\n", error));
    }
    output
}
