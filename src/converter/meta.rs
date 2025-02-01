use rust_norg::metadata::{parse_metadata, NorgMeta};
use std::str::FromStr;
use toml::{self, value::Datetime};
use eyre::{Error, Result};

fn parse_str_to_toml_value(s: &str) -> Result<toml::Value, MetaToTomlError> {
    if let Ok(datetime) = Datetime::from_str(s) {
        Ok(toml::Value::Datetime(datetime))
    } else if let Ok(bool_val) = s.parse::<bool>() {
        Ok(toml::Value::Boolean(bool_val))
    } else if let Ok(num) = s.parse::<f64>() {
        parse_number_to_toml_value(num)
    } else {
        Ok(toml::Value::String(s.into()))
    }
}

fn parse_number_to_toml_value(n: f64) -> Result<toml::Value, MetaToTomlError> {
    if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        Ok(toml::Value::Integer(n as i64))
    } else {
        Ok(toml::Value::Float(n))
    }
}

#[derive(Debug)]
enum MetaToTomlError {
    InvalidValue,
    EmptyKey,
}

impl std::fmt::Display for MetaToTomlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidValue => write!(f, "Invalid metadata value"),
            Self::EmptyKey => write!(f, "Empty key found"),
        }
    }
}

impl std::error::Error for MetaToTomlError {}

fn norg_meta_to_toml(meta: &NorgMeta) -> Result<toml::Value, MetaToTomlError> {
    match meta {
        NorgMeta::Bool(b) => Ok(toml::Value::Boolean(*b)),
        NorgMeta::Str(s) => parse_str_to_toml_value(s),
        NorgMeta::Num(n) => parse_number_to_toml_value(*n),
        NorgMeta::Array(arr) => {
            let mut items = Vec::new();
            for item in arr {
                items.push(norg_meta_to_toml(item)?);
            }
            Ok(toml::Value::Array(items))
        }
        NorgMeta::Object(obj) => {
            let mut table = toml::map::Map::new();
            for (key, value) in obj {
                table.insert(key.clone(), norg_meta_to_toml(value)?);
            }
            Ok(toml::Value::Table(table))
        }
        NorgMeta::Nil => Ok(toml::Value::String("nil".into())),
        NorgMeta::Invalid => Err(MetaToTomlError::InvalidValue),
        NorgMeta::EmptyKey(_) => Err(MetaToTomlError::EmptyKey),
    }
}

// Reuse the extract_meta function from previous implementation
fn extract_meta(input: &str) -> String {
    let mut in_meta = false;
    let mut result = Vec::new();

    for line in input.lines() {
        if line == "@document.meta" {
            in_meta = true;
            continue;
        }

        if in_meta {
            if line == "@end" {
                break;
            }
            result.push(line);
        }
    }

    result.join("\n")
}

/// Extracts and converts Norg metadata to TOML format
pub fn convert(document: &str) -> Result<toml::Value, Error> {
    let extracted_meta = extract_meta(document);
    let meta = parse_metadata(&extracted_meta)
        .expect("Failed to parse metadata");

    let toml_value = norg_meta_to_toml(&meta)
        .expect("Failed to convert metadata to TOML");

    Ok(toml_value)
}
