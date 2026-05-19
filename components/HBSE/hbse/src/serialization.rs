use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SerializationError {
    #[error("floats are forbidden in canonical protocol serialization")]
    FloatForbidden,
    #[error("top-level canonical value must be a JSON object")]
    TopLevelMustBeObject,
    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn b64url_no_padding(data: &[u8]) -> String {
    BASE64_URL_SAFE_NO_PAD.encode(data)
}

pub fn b64url_decode_no_padding(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64_URL_SAFE_NO_PAD.decode(data)
}

pub fn utc_millis(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, SerializationError> {
    let value = serde_json::to_value(value)?;
    match value {
        Value::Object(map) => {
            let normalized = normalize_value(Value::Object(map))?;
            Ok(serde_json::to_vec(&normalized)?)
        }
        _ => Err(SerializationError::TopLevelMustBeObject),
    }
}

pub fn canonical_json_string<T: Serialize>(value: &T) -> Result<String, SerializationError> {
    Ok(String::from_utf8(canonical_json_bytes(value)?).expect("serde_json emits UTF-8"))
}

fn normalize_value(value: Value) -> Result<Value, SerializationError> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(value),
        Value::Number(number) => {
            if number.is_f64() {
                Err(SerializationError::FloatForbidden)
            } else {
                Ok(Value::Number(number))
            }
        }
        Value::Array(items) => items
            .into_iter()
            .map(normalize_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut normalized = Map::new();
            for (key, value) in map {
                normalized.insert(key, normalize_value(value)?);
            }
            Ok(Value::Object(normalized))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys_and_removes_whitespace() {
        let value = json!({"z": 1, "a": {"b": true}, "m": [3, 2, 1]});

        assert_eq!(
            canonical_json_string(&value).unwrap(),
            r#"{"a":{"b":true},"m":[3,2,1],"z":1}"#
        );
    }

    #[test]
    fn canonical_json_rejects_floats() {
        let value = json!({"x": 1.5});

        assert!(matches!(
            canonical_json_bytes(&value),
            Err(SerializationError::FloatForbidden)
        ));
    }

    #[test]
    fn base64url_omits_padding() {
        assert_eq!(b64url_no_padding(b"hello"), "aGVsbG8");
        assert_eq!(b64url_decode_no_padding("aGVsbG8").unwrap(), b"hello");
    }
}
