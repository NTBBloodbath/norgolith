use std::collections::HashMap;

use eyre::Result;
use tera::{Error, Function, Value};

/// Now function
/// Template usage: {{ now(format="%A, %B %d") }} → "Thursday, October 05"
pub struct NowFunction;
impl Function for NowFunction {
    fn call(&self, args: &HashMap<String, Value>) -> Result<Value, Error> {
        let format = match args.get("format") {
            Some(v) => v
                .as_str()
                .ok_or(tera::Error::msg("`format` must be a string"))?,
            None => "%Y-%m-%d %H:%M:%S", // Default format
        };

        let now = chrono::Local::now();
        Ok(Value::String(now.format(format).to_string()))
    }
}
