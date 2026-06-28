use std::collections::HashMap;

use crate::schema::{MergedSchema, ValidationError};

pub fn validate_metadata(
    metadata: &HashMap<String, toml::Value>,
    merged: &MergedSchema,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check required fields
    for field in &merged.required {
        if !metadata.contains_key(field) {
            errors.push(ValidationError::MissingField(field.clone()));
        }
    }

    // Validate field types and constraints
    for (field, value) in metadata {
        if let Some(def) = merged.fields.get(field) {
            match def.validate(value, field) {
                Ok(_) => {}
                Err(e) => {
                    errors.push(e);
                }
            }
        }
    }

    // Apply conditional rules
    for rule in &merged.rules {
        match rule.applies(metadata) {
            Ok(true) => {
                if let Some(required) = &rule.then.required {
                    for field in required {
                        if !metadata.contains_key(field) {
                            errors.push(ValidationError::MissingField(field.clone()));
                        }
                    }
                }
            }
            Ok(false) => {} // Condition not met, do nothing
            Err(e) => errors.push(e),
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::schema::{
        FieldDefinition, MergedSchema, RuleAction, ValidationError, ValidationRule,
    };

    use super::validate_metadata;

    fn meta(pairs: &[(&str, toml::Value)]) -> HashMap<String, toml::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn required_only(fields: &[&str]) -> MergedSchema {
        MergedSchema {
            required: fields.iter().map(|s| s.to_string()).collect(),
            fields: HashMap::new(),
            rules: Vec::new(),
        }
    }

    // Required field checks

    #[test]
    fn missing_required_field_yields_error() {
        let merged = required_only(&["title"]);
        let errors = validate_metadata(&meta(&[]), &merged);
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ValidationError::MissingField(f) if f == "title"));
    }

    #[test]
    fn all_required_fields_present_is_ok() {
        let merged = required_only(&["title", "author"]);
        let errors = validate_metadata(
            &meta(&[
                ("title", toml::Value::String("My Post".into())),
                ("author", toml::Value::String("Alice".into())),
            ]),
            &merged,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn multiple_missing_required_fields_all_reported() {
        let merged = required_only(&["title", "author", "created"]);
        let errors = validate_metadata(
            &meta(&[("title", toml::Value::String("My Post".into()))]),
            &merged,
        );
        assert_eq!(errors.len(), 2);
    }

    // Field type and constraint checks

    #[test]
    fn type_mismatch_is_reported_with_field_name() {
        let mut merged = required_only(&[]);
        merged
            .fields
            .insert("draft".into(), FieldDefinition::Boolean);
        let errors = validate_metadata(
            &meta(&[("draft", toml::Value::String("true".into()))]),
            &merged,
        );
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ValidationError::TypeMismatch { field, .. } if field == "draft")
        );
    }

    #[test]
    fn constraint_violation_is_reported_with_field_name() {
        let mut merged = required_only(&[]);
        merged.fields.insert(
            "title".into(),
            FieldDefinition::String {
                max_length: Some(5),
                pattern: None,
            },
        );
        let errors = validate_metadata(
            &meta(&[(
                "title",
                toml::Value::String("This title is way too long".into()),
            )]),
            &merged,
        );
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ValidationError::ConstraintViolation { field, .. } if field == "title")
        );
    }

    #[test]
    fn unrecognized_field_is_not_an_error() {
        let merged = required_only(&[]);
        let errors = validate_metadata(
            &meta(&[("unknown_field", toml::Value::String("value".into()))]),
            &merged,
        );
        assert!(errors.is_empty());
    }

    // Conditional rule checks

    #[test]
    fn conditional_rule_triggers_required_check_when_met() {
        let mut merged = required_only(&[]);
        merged.rules.push(ValidationRule {
            condition: HashMap::from([("draft".into(), toml::Value::Boolean(false))]),
            then: RuleAction {
                required: Some(vec!["publish_date".into()]),
                fields: None,
            },
        });
        // draft = false and publish_date absent → error expected
        let errors = validate_metadata(&meta(&[("draft", toml::Value::Boolean(false))]), &merged);
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ValidationError::MissingField(f) if f == "publish_date"));
    }

    #[test]
    fn conditional_rule_skipped_when_condition_not_met() {
        let mut merged = required_only(&[]);
        merged.rules.push(ValidationRule {
            condition: HashMap::from([("draft".into(), toml::Value::Boolean(false))]),
            then: RuleAction {
                required: Some(vec!["publish_date".into()]),
                fields: None,
            },
        });
        // draft = true → condition not met, no error
        let errors = validate_metadata(&meta(&[("draft", toml::Value::Boolean(true))]), &merged);
        assert!(errors.is_empty());
    }

    #[test]
    fn conditional_rule_condition_field_absent_skips_rule() {
        let mut merged = required_only(&[]);
        merged.rules.push(ValidationRule {
            condition: HashMap::from([("draft".into(), toml::Value::Boolean(false))]),
            then: RuleAction {
                required: Some(vec!["publish_date".into()]),
                fields: None,
            },
        });
        // "draft" missing entirely -> condition not met, rule skipped, no errors
        let errors = validate_metadata(&meta(&[]), &merged);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn rule_required_field_present_when_condition_met_is_ok() {
        let mut merged = required_only(&[]);
        merged.rules.push(ValidationRule {
            condition: HashMap::from([("draft".into(), toml::Value::Boolean(false))]),
            then: RuleAction {
                required: Some(vec!["publish_date".into()]),
                fields: None,
            },
        });
        let errors = validate_metadata(
            &meta(&[
                ("draft", toml::Value::Boolean(false)),
                (
                    "publish_date",
                    toml::Value::String("2026-01-01T00:00:00Z".into()),
                ),
            ]),
            &merged,
        );
        assert!(errors.is_empty());
    }
}
