//! Exact-number validation for recursively parsed JSON values.

use crate::{JsonNumber, JsonNumberError};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// A rejected JSON number and its JSON Pointer location.
#[derive(Debug, Error)]
#[error("invalid JSON number at {path}: {source}")]
pub struct JsonNumberPathError {
    /// JSON Pointer locating the rejected member, or `/` for the root.
    pub path: String,
    /// The exact-number syntax or supported-range failure.
    #[source]
    pub source: JsonNumberError,
}

/// Validate and canonicalize every number in a parsed JSON document.
///
/// This walk runs before NGSI-LD deserialization and JSON-LD expansion, so
/// scalar, compound, list, and `@json` values all enter those layers in the
/// same exact decimal domain.
///
/// # Errors
///
/// Returns [`JsonNumberPathError`] at the first number outside the supported
/// PostgreSQL `NUMERIC` range.
pub fn canonicalize_numbers(value: &mut Value) -> Result<(), JsonNumberPathError> {
    canonicalize_at(value, String::new())
}

/// Deserialize a domain value from an arbitrary-precision JSON value.
///
/// `serde_json::from_value` routes integers above `u64::MAX` through Serde's
/// `u128` visitor, which nested `serde_json::Value` fields do not accept. A
/// `serde_json` serialization followed by its streaming deserializer keeps the
/// same bare number token and supports that full domain without a float or an
/// incompatible serializer.
///
/// # Errors
///
/// Returns the domain deserialization error, or the unlikely serialization
/// error if the supplied value cannot be emitted as JSON.
pub fn deserialize_exact<T: DeserializeOwned>(value: &Value) -> Result<T, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    serde_json::from_slice(&bytes)
}

/// Canonicalize a value while maintaining its JSON Pointer location.
fn canonicalize_at(value: &mut Value, path: String) -> Result<(), JsonNumberPathError> {
    match value {
        Value::Number(number) => {
            let exact = JsonNumber::try_from(&*number).map_err(|source| JsonNumberPathError {
                path: display_path(path),
                source,
            })?;
            *number = exact.to_serde_number();
            Ok(())
        }
        Value::Array(items) => items
            .iter_mut()
            .enumerate()
            .try_for_each(|(index, item)| canonicalize_at(item, append_segment(&path, &index.to_string()))),
        Value::Object(members) => members
            .iter_mut()
            .try_for_each(|(name, member)| canonicalize_at(member, append_segment(&path, &escape_pointer_segment(name)))),
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}

/// Append one escaped segment to a JSON Pointer.
fn append_segment(path: &str, segment: &str) -> String {
    format!("{path}/{segment}")
}

/// Escape a member name according to RFC 6901.
fn escape_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Give the root pointer an unambiguous printable spelling.
fn display_path(path: String) -> String {
    if path.is_empty() { "/".to_string() } else { path }
}

#[cfg(test)]
mod tests {
    use crate::json_value::{canonicalize_numbers, deserialize_exact};
    use serde_json::Value;

    #[test]
    fn nested_property_list_and_json_numbers_remain_exact() {
        let mut value: Value = serde_json::from_str(
            r#"{"property":{"value":0.100000000000000005,"nested":[18446744073709551616]},"valueList":[-2.5e-400],"json":{"n":1.234567890123456789e20}}"#,
        )
        .expect("valid arbitrary-precision JSON");
        canonicalize_numbers(&mut value).expect("all numbers are supported");
        let output = serde_json::to_string(&value).expect("canonical JSON");
        assert!(output.contains("0.100000000000000005"), "got {output}");
        assert!(output.contains("18446744073709551616"), "got {output}");
        assert!(output.contains("-2.5E-400"), "got {output}");
        assert!(output.contains("123456789012345678900"), "got {output}");
    }

    #[test]
    fn an_out_of_range_member_reports_its_json_pointer() {
        let mut value: Value = serde_json::from_str(r#"{"property":{"value":[0,{"too/large":1e-16384}]}}"#).expect("valid JSON");
        let error = canonicalize_numbers(&mut value).expect_err("the fractional range is exceeded");
        pretty_assertions::assert_eq!(error.path, "/property/value/1/too~1large");
    }

    #[test]
    fn value_to_domain_deserialization_keeps_integers_above_u64() {
        let value: Value = serde_json::from_str(r#"{"number":18446744073709551616}"#).expect("arbitrary-precision JSON");
        let decoded: Value = deserialize_exact(&value).expect("streaming deserialization accepts the number");
        pretty_assertions::assert_eq!(serde_json::to_string(&decoded).expect("JSON"), r#"{"number":18446744073709551616}"#);
    }
}
