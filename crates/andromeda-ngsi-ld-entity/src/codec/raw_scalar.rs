//! The content of a value object's `@value`, read without knowing yet whether
//! the object is a literal or a JSON literal.
//!
//! JSON-LD orders nothing inside an object, so `@type: @json` may come after
//! `@value`. The content is therefore read as whatever JSON it is: a scalar
//! stays a scalar, and an object, an array or `null` is kept as the JSON it is
//! for a JSON literal. Numbers keep their exact token (`arbitrary_precision`).

use serde::{
    Deserialize,
    Deserializer,
    de::{
        Error,
        MapAccess,
        SeqAccess,
        Visitor,
        value::{MapAccessDeserializer, SeqAccessDeserializer},
    },
};
use serde_json::{Number, Value};
use std::fmt;

/// The content of one `@value`.
#[derive(Debug)]
pub(crate) enum RawScalar {
    /// A number, with its exact token.
    Number(Number),
    /// A string.
    String(String),
    /// A boolean.
    Boolean(bool),
    /// An object, an array or `null`: only a JSON literal can hold it.
    Json(Value),
}

impl RawScalar {
    /// The content as the JSON value a JSON literal carries.
    pub(crate) fn into_json(self) -> Value {
        match self {
            RawScalar::Number(number) => Value::Number(number),
            RawScalar::String(text) => Value::String(text),
            RawScalar::Boolean(flag) => Value::Bool(flag),
            RawScalar::Json(value) => value,
        }
    }
}

impl<'de> Deserialize<'de> for RawScalar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RawScalar, D::Error> {
        deserializer.deserialize_any(RawScalarVisitor)
    }
}

/// Reads whatever JSON stands in `@value`.
struct RawScalarVisitor;

impl<'de> Visitor<'de> for RawScalarVisitor {
    type Value = RawScalar;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, flag: bool) -> Result<RawScalar, E> {
        Ok(RawScalar::Boolean(flag))
    }

    fn visit_u64<E>(self, number: u64) -> Result<RawScalar, E> {
        Ok(RawScalar::Number(Number::from(number)))
    }

    fn visit_i64<E>(self, number: i64) -> Result<RawScalar, E> {
        Ok(RawScalar::Number(Number::from(number)))
    }

    fn visit_f64<E: Error>(self, number: f64) -> Result<RawScalar, E> {
        Number::from_f64(number)
            .map(RawScalar::Number)
            .ok_or_else(|| E::custom("a JSON number is finite"))
    }

    fn visit_str<E>(self, text: &str) -> Result<RawScalar, E> {
        Ok(RawScalar::String(text.to_owned()))
    }

    fn visit_string<E>(self, text: String) -> Result<RawScalar, E> {
        Ok(RawScalar::String(text))
    }

    fn visit_unit<E>(self) -> Result<RawScalar, E> {
        Ok(RawScalar::Json(Value::Null))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RawScalar, A::Error> {
        // With `arbitrary_precision` a number that is not a 64-bit integer
        // arrives as a one-entry map; `Value` knows how to read that back as
        // the number it is, so the map is handed to it rather than parsed here.
        match Value::deserialize(MapAccessDeserializer::new(map))? {
            Value::Number(number) => Ok(RawScalar::Number(number)),
            other @ (Value::Null | Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => Ok(RawScalar::Json(other)),
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<RawScalar, A::Error> {
        Value::deserialize(SeqAccessDeserializer::new(seq)).map(RawScalar::Json)
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::raw_scalar::RawScalar;
    #[test]
    fn a_long_decimal_keeps_its_token() {
        let scalar: RawScalar = serde_json::from_str("123456789012345678901234567890.5").expect("a number");
        assert!(matches!(scalar, RawScalar::Number(number) if number.as_str() == "123456789012345678901234567890.5"));
    }

    #[test]
    fn an_object_is_kept_as_json() {
        let scalar: RawScalar = serde_json::from_str(r#"{"a": 1.50}"#).expect("an object");
        let expected: serde_json::Value = serde_json::from_str(r#"{"a": 1.50}"#).expect("an object");
        assert!(matches!(scalar, RawScalar::Json(value) if value == expected));
    }
}
