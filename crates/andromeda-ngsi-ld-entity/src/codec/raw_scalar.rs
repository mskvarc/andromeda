//! The content of a value object's `@value`, read without knowing yet whether
//! the object is a literal or a JSON literal.
//!
//! JSON-LD orders nothing inside an object, so `@type: @json` may come after
//! `@value`. The content is therefore kept as the text it was written as, and
//! read as a literal or admitted as a JSON literal once the object is closed.
//! A string is unescaped as it is read, because the NGSI-LD Null is recognised
//! in `@value` whatever the object turns out to be.

use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_number::JsonNumber;
use serde::{Deserialize, Deserializer, de::Error};
use serde_json::value::RawValue;
use std::str::FromStr;

/// The content of one `@value`.
#[derive(Debug)]
pub(crate) struct RawScalar<'de> {
    /// The content as written.
    raw: &'de RawValue,
    /// The content unescaped, when it is a string.
    text: Option<String>,
}

/// The content of one `@value` read as a literal clause 4.6.3 can hold.
#[derive(Debug)]
pub(crate) enum ScalarContent {
    /// An exact number.
    Number(JsonNumber),
    /// A string.
    String(String),
    /// A boolean.
    Boolean(bool),
}

impl RawScalar<'_> {
    /// The content when it is a string.
    pub(crate) fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// The content when it is a string, given up.
    pub(crate) fn into_text(self) -> Option<String> {
        self.text
    }

    /// The content as the nearest binary64 value, when it is a number.
    pub(crate) fn as_f64(&self) -> Option<f64> {
        self.number_text().and_then(|text| text.parse().ok())
    }

    /// The content's text when it is a number.
    fn number_text(&self) -> Option<&str> {
        let text = self.raw.get();
        text.starts_with(['-', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9']).then_some(text)
    }

    /// The content as the JSON literal a `@json` value object carries.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLiteralError`] admission raises.
    pub(crate) fn into_json_literal(self) -> Result<JsonLiteral, JsonLiteralError> {
        JsonLiteral::admit(self.raw.get())
    }

    /// The content as a literal: `None` for an object, an array, `null` or a
    /// number outside the supported range, none of which a plain value object
    /// carries.
    pub(crate) fn into_scalar(self) -> Option<ScalarContent> {
        if let Some(number) = self.number_text() {
            return JsonNumber::from_str(number).ok().map(ScalarContent::Number);
        }
        if let Some(text) = self.text {
            return Some(ScalarContent::String(text));
        }
        match self.raw.get() {
            "true" => Some(ScalarContent::Boolean(true)),
            "false" => Some(ScalarContent::Boolean(false)),
            // `&str` is unbounded; anything else is `null`, an object or an
            // array.
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for RawScalar<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RawScalar<'de>, D::Error> {
        let raw = <&'de RawValue>::deserialize(deserializer)?;
        let text = if raw.get().starts_with('"') {
            Some(serde_json::from_str::<String>(raw.get()).map_err(D::Error::custom)?)
        } else {
            None
        };
        Ok(RawScalar { raw, text })
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::raw_scalar::{RawScalar, ScalarContent};

    #[test]
    fn a_long_decimal_keeps_its_value() {
        let scalar: RawScalar<'_> = serde_json::from_str("123456789012345678901234567890.5").expect("a number");
        assert!(matches!(scalar.into_scalar(), Some(ScalarContent::Number(number)) if number.to_string() == "123456789012345678901234567890.5"));
    }

    #[test]
    fn an_object_is_kept_as_its_text() {
        let scalar: RawScalar<'_> = serde_json::from_str(r#"{"a": 1.50}"#).expect("an object");
        pretty_assertions::assert_eq!(scalar.into_json_literal().expect("a JSON literal").text(), r#"{"a":1.5}"#);
    }

    #[test]
    fn a_string_is_unescaped_once() {
        let scalar: RawScalar<'_> = serde_json::from_str(r#""a\"b""#).expect("a string");
        pretty_assertions::assert_eq!(scalar.text(), Some("a\"b"));
        assert!(matches!(scalar.into_scalar(), Some(ScalarContent::String(text)) if text == "a\"b"));
    }
}
