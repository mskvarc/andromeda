//! Exact-number validation for recursively parsed JSON values.

mod decode_route;
pub mod json_pointer;
pub mod number_range;
pub mod value_census;

use crate::{
    JsonNumber,
    JsonNumberError,
    json_value::{
        decode_route::{DecodeRoute, document_decode_route, typed_decode_route},
        json_pointer::{JsonPointer, PathSegment},
    },
    number_token::NumberToken,
};
use serde::de::DeserializeOwned;
use serde_json::{Number, Value};
use thiserror::Error;

/// The deepest container nesting `serde_json`'s reader accepts in one text:
/// its recursion budget of 128 is spent on entering each container, and
/// refused at zero. Every request body and every stored document is read with
/// that reader and that budget.
pub const READER_NESTING_LIMIT: usize = 127;

/// A rejected JSON number and its JSON Pointer location.
#[derive(Debug, Error)]
#[error("invalid JSON number at {path}: {source}")]
pub struct JsonNumberPathError {
    /// JSON Pointer locating the rejected member.
    pub path: JsonPointer,
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
    canonicalize_at(value, &mut Vec::new())
}

/// Deserialize a domain value from an arbitrary-precision JSON value.
///
/// The result is always the one `serde_json`'s streaming reader gives for the
/// value's serialization: that reader hands every number that is not a 64-bit
/// integer to the visitor as a bare token, which keeps integers above
/// `u64::MAX` and non-binary64 decimals exact inside nested `Value` fields and
/// serde's buffered untagged and flattened content, where the value
/// deserializer's native `u128` and `f64` visits are refused or respelled. A
/// value the value deserializer provably decodes the same way is decoded in
/// memory; any other value is serialized and read back.
///
/// # Errors
///
/// Returns the domain deserialization error, positioned in the value's
/// serialization as the streaming reader reports it, or the unlikely
/// serialization error if the supplied value cannot be emitted as JSON.
pub fn deserialize_exact<T: DeserializeOwned>(value: &Value) -> Result<T, serde_json::Error> {
    match typed_decode_route(value) {
        // A rejection carries the reader's line and column of the failing
        // token, which only reading the serialization can report.
        DecodeRoute::Direct => T::deserialize(value).or_else(|_unpositioned| read_serialization(value)),
        DecodeRoute::ByteRoundTrip => read_serialization(value),
    }
}

/// Settle an untyped JSON document into the exact `Value` that
/// [`deserialize_exact`] gives for it.
///
/// A canonicalized document is already in that shape and is returned as it
/// is; only a document whose serialization the streaming reader would
/// respell or reinterpret is serialized and read back.
///
/// # Errors
///
/// Returns the streaming reader's error for a document it refuses, or the
/// unlikely serialization error if the document cannot be emitted as JSON.
pub fn deserialize_exact_document(document: Value) -> Result<Value, serde_json::Error> {
    match document_decode_route(&document) {
        DecodeRoute::Direct => Ok(document),
        DecodeRoute::ByteRoundTrip => read_serialization(&document),
    }
}

/// Serialize a value and deserialize the result with the streaming reader.
fn read_serialization<T: DeserializeOwned>(value: &Value) -> Result<T, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    serde_json::from_slice(&bytes)
}

/// Canonicalize a value while maintaining its JSON Pointer location.
fn canonicalize_at<'a>(value: &'a mut Value, path: &mut Vec<PathSegment<'a>>) -> Result<(), JsonNumberPathError> {
    match value {
        Value::Number(number) => canonicalize_number(number, path),
        Value::Array(items) => items.iter_mut().enumerate().try_for_each(|(index, item)| {
            path.push(PathSegment::Index(index));
            let outcome = canonicalize_at(item, path);
            path.pop();
            outcome
        }),
        Value::Object(members) => members.iter_mut().try_for_each(|(name, member)| {
            path.push(PathSegment::Member(name));
            let outcome = canonicalize_at(member, path);
            path.pop();
            outcome
        }),
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}

/// Validate one number and give it the spelling [`JsonNumber`] renders.
///
/// A token already in that spelling whose digits lie within range is left as
/// it is without building its decimal, which is what keeps a document of
/// millions of coordinates cheap to admit.
fn canonicalize_number(number: &mut Number, path: &[PathSegment<'_>]) -> Result<(), JsonNumberPathError> {
    if NumberToken::parse(number.as_str()).is_some_and(|token| token.is_canonical_within_range()) {
        return Ok(());
    }
    let exact = JsonNumber::try_from(&*number).map_err(|source| JsonNumberPathError {
        path: JsonPointer::from_walk(path),
        source,
    })?;
    if !NumberToken::parse(number.as_str()).is_some_and(|token| token.is_canonical()) {
        *number = exact.to_serde_number();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        JsonNumber,
        json_value::{canonicalize_numbers, deserialize_exact, deserialize_exact_document, read_serialization},
    };
    use serde::Deserialize;
    use serde_json::{Number, Value, json};
    use std::{collections::BTreeMap, fmt::Debug};

    /// A decode target exercising every visitor path a number can take:
    /// exact numbers, primitive hints, nested `Value`s, untagged content and
    /// flattened content.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Probe {
        #[serde(default)]
        exact: Option<JsonNumber>,
        #[serde(default)]
        float: Option<f64>,
        #[serde(default)]
        small: Option<u8>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        either: Option<Either>,
        #[serde(default)]
        document: Option<Value>,
        #[serde(flatten)]
        rest: BTreeMap<String, Value>,
    }

    /// An untagged choice that buffers its input before picking a variant.
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(untagged)]
    enum Either {
        Integer(i64),
        Float(f64),
        Text(String),
        Document(Value),
    }

    /// Numbers covering 64-bit boundaries, wide integers, exact and inexact
    /// binary64 decimals, both exponent spellings, negative zero, and tokens
    /// outside the supported range of primitive fields.
    const NUMBERS: [&str; 22] = [
        "0",
        "-0",
        "7",
        "-7",
        "300",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "340282366920938463463374607431768211456",
        "21.5",
        "0.1",
        "0.00001",
        "0.100000000000000005",
        "1e400",
        "1E-7",
        "1e-7",
        "-2.5E-400",
        "123456789012345678900",
        "1.234567890123456789e20",
        "+5",
        "007",
    ];

    /// A JSON number carrying `token` verbatim. `json!` cannot embed one: it
    /// re-serializes interpolated values, which respells the token.
    fn number(token: &str) -> Value {
        Value::Number(Number::from_string_unchecked(token.to_owned()))
    }

    /// An object with these members, embedded without re-serialization.
    fn object<const N: usize>(members: [(&str, Value); N]) -> Value {
        Value::Object(members.into_iter().map(|(name, value)| (name.to_owned(), value)).collect())
    }

    /// An array of these items, embedded without re-serialization.
    fn array<const N: usize>(items: [Value; N]) -> Value {
        Value::Array(Vec::from(items))
    }

    /// `depth` arrays nested inside one another around `inner`.
    fn nested_arrays(depth: usize, inner: Value) -> Value {
        (0..depth).fold(inner, |inner, _| Value::Array(vec![inner]))
    }

    /// Documents placing every number in every probe position, plus the
    /// member-name and nesting edge cases.
    fn documents() -> Vec<Value> {
        let mut documents = NUMBERS
            .iter()
            .flat_map(|token| {
                ["exact", "float", "small", "text", "either", "document", "extra"]
                    .map(|name| object([(name, number(token))]))
                    .into_iter()
                    .chain([
                        object([
                            ("document", object([("nested", array([number(token)]))])),
                            ("either", object([("n", number(token))])),
                        ]),
                        object([("extra", array([number(token), object([("n", number(token))])]))]),
                        array([number(token)]),
                        number(token),
                    ])
            })
            .collect::<Vec<_>>();
        documents.extend([
            json!({}),
            json!({"text": "plain", "small": 5, "either": "a", "document": {"list": [1, -2, "x", null, true]}}),
            json!({"text": 5}),
            json!({"small": "5"}),
            json!({"document": {"$serde_json::private::Number": "5"}}),
            json!({"document": {"$serde_json::private::Number": "5", "a": 1}}),
            json!({"document": {"$serde_json::private::Number": 5}}),
            json!({"document": {"$serde_json::private::RawValue": "[1]"}}),
            json!({"either": {"$serde_json::private::Number": "0.5"}}),
            object([("document", nested_arrays(125, number("1")))]),
            object([("document", nested_arrays(126, number("1")))]),
            object([("document", nested_arrays(127, number("0.5")))]),
        ]);
        documents
    }

    /// A decode target whose one required member rejects a number.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Named {
        text: String,
    }

    /// Assert two decode outcomes of the document spelled `document` are the
    /// same value or the same error text.
    fn assert_same_outcome<T: PartialEq + Debug>(actual: Result<T, serde_json::Error>, expected: Result<T, serde_json::Error>, document: &str) {
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => pretty_assertions::assert_eq!(actual, expected, "{document}"),
            (Err(actual), Err(expected)) => pretty_assertions::assert_eq!(actual.to_string(), expected.to_string(), "{document}"),
            (actual, expected) => panic!("{document}: decoded {actual:?}, the streaming reader gives {expected:?}"),
        }
    }

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
    fn every_number_takes_the_spelling_json_number_renders() {
        for token in NUMBERS {
            let Ok(exact) = JsonNumber::try_from(Number::from_string_unchecked(token.to_owned())) else {
                continue;
            };
            let mut value = object([("n", array([number(token)]))]);
            canonicalize_numbers(&mut value).expect("a supported number");
            pretty_assertions::assert_eq!(value, object([("n", array([exact.to_json_value()]))]), "{token}");
        }
    }

    #[test]
    fn an_out_of_range_member_reports_its_json_pointer() {
        let mut value: Value = serde_json::from_str(r#"{"property":{"value":[0,{"too/large":1e-16384}]}}"#).expect("valid JSON");
        let error = canonicalize_numbers(&mut value).expect_err("the fractional range is exceeded");
        pretty_assertions::assert_eq!(error.path.to_string(), "/property/value/1/too~1large");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "invalid JSON number at /property/value/1/too~1large: JSON number exceeds the supported range of 131072 integral digits and 16383 fractional digits"
        );
    }

    #[test]
    fn an_out_of_range_root_number_reports_the_root_pointer() {
        let mut value = number("1e-16384");
        let error = canonicalize_numbers(&mut value).expect_err("the fractional range is exceeded");
        pretty_assertions::assert_eq!(error.path.to_string(), "/");
    }

    #[test]
    fn a_member_name_with_a_tilde_is_escaped_in_the_reported_pointer() {
        let mut value = object([("a~b", object([("c/d~", array([number("1"), number("2"), number("1e-16384")]))]))]);
        let error = canonicalize_numbers(&mut value).expect_err("the fractional range is exceeded");
        pretty_assertions::assert_eq!(error.path.to_string(), "/a~0b/c~1d~0/2");
    }

    #[test]
    fn value_to_domain_deserialization_keeps_integers_above_u64() {
        let value: Value = serde_json::from_str(r#"{"number":18446744073709551616}"#).expect("arbitrary-precision JSON");
        let decoded: Value = deserialize_exact(&value).expect("streaming deserialization accepts the number");
        pretty_assertions::assert_eq!(serde_json::to_string(&decoded).expect("JSON"), r#"{"number":18446744073709551616}"#);
    }

    #[test]
    fn typed_decoding_matches_the_streaming_reader_for_every_edge_case() {
        for document in documents() {
            let spelled = document.to_string();
            assert_same_outcome(deserialize_exact::<Probe>(&document), read_serialization::<Probe>(&document), &spelled);
            assert_same_outcome(deserialize_exact::<Either>(&document), read_serialization::<Either>(&document), &spelled);
            assert_same_outcome(deserialize_exact::<Named>(&document), read_serialization::<Named>(&document), &spelled);
            assert_same_outcome(deserialize_exact::<Value>(&document), read_serialization::<Value>(&document), &spelled);
        }
    }

    #[test]
    fn a_rejected_typed_decode_keeps_the_streaming_readers_positioned_message() {
        let document = json!({"text": 5});
        let error = deserialize_exact::<Named>(&document).expect_err("a number is not a string");
        let expected = read_serialization::<Named>(&document).expect_err("a number is not a string");
        pretty_assertions::assert_eq!(error.to_string(), expected.to_string());
        assert!(error.to_string().contains(" at line 1 column "), "{error}");
    }

    #[test]
    fn document_decoding_matches_the_streaming_reader_for_every_edge_case() {
        for document in documents() {
            let spelled = document.to_string();
            let expected = read_serialization::<Value>(&document);
            assert_same_outcome(deserialize_exact_document(document), expected, &spelled);
        }
    }

    #[test]
    fn document_decoding_matches_the_streaming_reader_after_canonicalization() {
        for mut document in documents() {
            if canonicalize_numbers(&mut document).is_ok() {
                let spelled = document.to_string();
                let expected = read_serialization::<Value>(&document);
                assert_same_outcome(deserialize_exact_document(document), expected, &spelled);
            }
        }
    }

    #[test]
    fn a_canonicalized_document_is_settled_without_being_respelled() {
        let mut document: Value = serde_json::from_str(r#"{"a":[21.5,-7,18446744073709551616,0.100000000000000005],"b":{"c":"text"}}"#).expect("valid JSON");
        canonicalize_numbers(&mut document).expect("supported numbers");
        let spelled = document.to_string();
        let settled = deserialize_exact_document(document).expect("a readable document");
        pretty_assertions::assert_eq!(settled.to_string(), spelled);
    }
}
