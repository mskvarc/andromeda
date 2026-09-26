//! Which JSON values decode the same from memory as from their serialization.
//!
//! With `arbitrary_precision`, `serde_json`'s value deserializer and its
//! streaming reader hand numbers to a visitor differently. The reader passes a
//! 64-bit integer as one and every other number as an opaque token; the value
//! deserializer also passes 128-bit integers and numbers an `f64` spells
//! exactly as native integers and floats, and parses a number straight from
//! its text when the visitor asks for a primitive. Buffered visitors -- serde's
//! untagged and flattened content, a nested `Value` -- and primitive fields can
//! therefore decode those numbers differently. The reader additionally refuses
//! containers nested past its recursion limit, and treats an object whose
//! first member carries one of `serde_json`'s private token names as that token.
//! A value clear of every one of those differences decodes identically either
//! way; any other value has to be decoded from its serialization.

use crate::number_token::NumberToken;
use serde_json::{Map, Number, Value};

/// The deepest container nesting `serde_json`'s reader accepts: its recursion
/// budget of 128 is spent on entering each container, and refused at zero.
const READER_NESTING_LIMIT: usize = 127;

/// The prefix of the member names `serde_json`'s value visitor reserves for its
/// arbitrary-precision number and raw-value tokens.
const SERDE_JSON_PRIVATE_MEMBER_PREFIX: &str = "$serde_json::private::";

/// How a JSON value has to be decoded to give the result `serde_json`'s reader
/// gives for its serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeRoute {
    /// Decoding the value in memory gives the reader's result.
    Direct,
    /// Only reading the value's serialization gives the reader's result.
    ByteRoundTrip,
}

/// The route for decoding a value into an arbitrary domain type.
///
/// Every number has to be a 64-bit integer, the only numbers both
/// deserializers hand to every visitor the same way. Private member names need
/// no check: both deserializers visit an object's members in the same order
/// through the same value visitor.
pub(crate) fn typed_decode_route(value: &Value) -> DecodeRoute {
    route_within(value, Criteria::Typed, 0)
}

/// The route for decoding a value into an untyped `Value`.
///
/// A `Value` target sees every number as the reader spells it, so each number
/// has to read back unchanged, and no member may carry a private token name
/// the reader could reinterpret.
pub(crate) fn document_decode_route(value: &Value) -> DecodeRoute {
    route_within(value, Criteria::Document, 0)
}

/// What a value has to satisfy for one kind of decode target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Criteria {
    /// Decoding into an arbitrary domain type.
    Typed,
    /// Decoding into an untyped `Value`.
    Document,
}

impl Criteria {
    /// Whether a number decodes identically in memory and from its text.
    fn admits_number(self, number: &Number) -> bool {
        NumberToken::parse(number.as_str()).is_some_and(|token| match self {
            Criteria::Typed => token.is_64_bit_integer(),
            Criteria::Document => token.reads_back_unchanged(),
        })
    }

    /// Whether a member name decodes identically in memory and from its text.
    fn admits_member_name(self, name: &str) -> bool {
        match self {
            Criteria::Typed => true,
            Criteria::Document => !name.starts_with(SERDE_JSON_PRIVATE_MEMBER_PREFIX),
        }
    }
}

/// The route for `value`, found inside `enclosing` containers.
fn route_within(value: &Value, criteria: Criteria, enclosing: usize) -> DecodeRoute {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => DecodeRoute::Direct,
        Value::Number(number) if criteria.admits_number(number) => DecodeRoute::Direct,
        Value::Number(_) => DecodeRoute::ByteRoundTrip,
        Value::Array(_) | Value::Object(_) if enclosing >= READER_NESTING_LIMIT => DecodeRoute::ByteRoundTrip,
        Value::Array(items) => route_of_all(items.iter(), criteria, enclosing + 1),
        Value::Object(members) => route_of_members(members, criteria, enclosing + 1),
    }
}

/// The route for an object's members, found inside `enclosing` containers.
fn route_of_members(members: &Map<String, Value>, criteria: Criteria, enclosing: usize) -> DecodeRoute {
    if members.keys().all(|name| criteria.admits_member_name(name)) {
        route_of_all(members.values(), criteria, enclosing)
    } else {
        DecodeRoute::ByteRoundTrip
    }
}

/// The route for sibling values, each found inside `enclosing` containers.
fn route_of_all<'a>(values: impl IntoIterator<Item = &'a Value>, criteria: Criteria, enclosing: usize) -> DecodeRoute {
    values
        .into_iter()
        .map(|value| route_within(value, criteria, enclosing))
        .find(|route| *route == DecodeRoute::ByteRoundTrip)
        .unwrap_or(DecodeRoute::Direct)
}

#[cfg(test)]
mod tests {
    use crate::json_value::decode_route::{DecodeRoute, READER_NESTING_LIMIT, document_decode_route, typed_decode_route};
    use serde_json::{Map, Number, Value, json};

    /// `depth` arrays nested inside one another.
    fn nested_arrays(depth: usize) -> Value {
        (0..depth).fold(Value::Null, |inner, _| Value::Array(vec![inner]))
    }

    /// A JSON number carrying `token` verbatim. `json!` cannot embed one: it
    /// re-serializes interpolated values, which respells the token.
    fn number(token: &str) -> Value {
        Value::Number(Number::from_string_unchecked(token.to_owned()))
    }

    /// An object with the single member `name`.
    fn member(name: &str, value: Value) -> Value {
        Value::Object(Map::from_iter([(name.to_owned(), value)]))
    }

    #[test]
    fn a_document_of_strings_and_64_bit_integers_decodes_directly_for_both_targets() {
        let value = json!({"id": "urn:ngsi-ld:T:1", "count": {"value": [0, -5, 18_446_744_073_709_551_615_u64, i64::MIN]}, "flag": true, "none": null});
        pretty_assertions::assert_eq!(typed_decode_route(&value), DecodeRoute::Direct);
        pretty_assertions::assert_eq!(document_decode_route(&value), DecodeRoute::Direct);
    }

    #[test]
    fn a_fraction_or_wide_integer_sends_a_typed_decode_through_the_round_trip() {
        for token in ["0.1", "18446744073709551616", "-9223372036854775809", "1e+2"] {
            let value = member("nested", Value::Array(vec![member("n", number(token))]));
            pretty_assertions::assert_eq!(typed_decode_route(&value), DecodeRoute::ByteRoundTrip, "{token}");
            pretty_assertions::assert_eq!(document_decode_route(&value), DecodeRoute::Direct, "{token}");
        }
    }

    #[test]
    fn a_number_the_reader_respells_sends_a_document_decode_through_the_round_trip() {
        for token in ["-2.5E-400", "1e2", "1E+2", "-0"] {
            let value = Value::Array(vec![number(token)]);
            pretty_assertions::assert_eq!(document_decode_route(&value), DecodeRoute::ByteRoundTrip, "{token}");
        }
    }

    #[test]
    fn a_number_outside_json_grammar_sends_both_decodes_through_the_round_trip() {
        for token in ["+5", "007", "NaN"] {
            let value = Value::Array(vec![number(token)]);
            pretty_assertions::assert_eq!(typed_decode_route(&value), DecodeRoute::ByteRoundTrip, "{token}");
            pretty_assertions::assert_eq!(document_decode_route(&value), DecodeRoute::ByteRoundTrip, "{token}");
        }
    }

    #[test]
    fn a_private_serde_json_member_name_sends_only_a_document_decode_through_the_round_trip() {
        let value = json!({"property": {"$serde_json::private::Number": "5", "a": 1}});
        pretty_assertions::assert_eq!(typed_decode_route(&value), DecodeRoute::Direct);
        pretty_assertions::assert_eq!(document_decode_route(&value), DecodeRoute::ByteRoundTrip);
    }

    #[test]
    fn nesting_past_the_reader_limit_sends_both_decodes_through_the_round_trip() {
        let deepest_accepted = nested_arrays(READER_NESTING_LIMIT);
        assert!(serde_json::from_str::<Value>(&deepest_accepted.to_string()).is_ok());
        pretty_assertions::assert_eq!(typed_decode_route(&deepest_accepted), DecodeRoute::Direct);
        pretty_assertions::assert_eq!(document_decode_route(&deepest_accepted), DecodeRoute::Direct);

        let too_deep = nested_arrays(READER_NESTING_LIMIT + 1);
        assert!(serde_json::from_str::<Value>(&too_deep.to_string()).is_err());
        pretty_assertions::assert_eq!(typed_decode_route(&too_deep), DecodeRoute::ByteRoundTrip);
        pretty_assertions::assert_eq!(document_decode_route(&too_deep), DecodeRoute::ByteRoundTrip);
    }
}
