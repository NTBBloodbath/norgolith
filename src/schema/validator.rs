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
            match def.validate(value) {
                Ok(_) => {}
                Err(mut e) => {
                    e.with_field(field.to_string());
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
