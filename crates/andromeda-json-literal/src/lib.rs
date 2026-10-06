//! A JSON value carried as data rather than interpreted.
//!
//! JSON-LD 1.1 keeps the value of a term coerced to `@json` out of expansion
//! and compaction (JSON-LD 1.1 section 4.2.2), and NGSI-LD clause 4.5.24
//! carries the `json` member of a `JsonProperty` that way: "data which is not
//! available for JSON-LD interpretation". A `GeoJSON` foreign member (IETF RFC
//! 7946 section 6.1) is the same kind of value. Such a value is held as its
//! admitted JSON text, never as a tree: it is read in place when something
//! needs to look inside it, and written out verbatim.

pub mod admission;
pub mod equality;
pub mod json_text;
pub mod merge_patch;
pub mod null_sentinel;
pub mod piece;
pub mod processor_json;
mod transcription;

use crate::{
    admission::{JsonLiteralError, canonical_text},
    equality::same_value,
    json_text::quoted,
    piece::JsonPiece,
    transcription::transcribed,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};
use serde_json::value::RawValue;
use std::{fmt, str::FromStr};

/// A JSON value carried as data: a `JsonProperty` value, an `@json` literal,
/// a `GeoJSON` foreign member.
///
/// The text is admitted once: every number is spelled the canonical way
/// [`JsonNumber`](andromeda_ngsi_ld_number::JsonNumber) spells it, an object
/// never names a member twice, nothing nests deeper than the reader accepts,
/// and members keep the order they were written in. Equality is the JSON data
/// model's: member order is ignored and numbers compare by value.
#[derive(Clone, Debug)]
pub struct JsonLiteral(Box<RawValue>);

impl JsonLiteral {
    /// Admit one JSON text.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError`] when the text is not one well-formed JSON
    /// value, names a member twice in one object, carries a number outside the
    /// supported range, or nests too deep.
    pub fn admit(text: &str) -> Result<JsonLiteral, JsonLiteralError> {
        let raw: &RawValue = serde_json::from_str(text).map_err(|source| JsonLiteralError::Syntax { source })?;
        JsonLiteral::admit_raw(raw)
    }

    /// Admit the text of one raw JSON value.
    ///
    /// # Errors
    ///
    /// As [`JsonLiteral::admit`], for text that is one JSON value already.
    pub fn admit_raw(raw: &RawValue) -> Result<JsonLiteral, JsonLiteralError> {
        JsonLiteral::from_admitted_text(canonical_text(raw)?)
    }

    /// Hold text this crate wrote from admitted pieces.
    fn from_admitted_text(text: String) -> Result<JsonLiteral, JsonLiteralError> {
        RawValue::from_string(text)
            .map(JsonLiteral)
            .map_err(|source| JsonLiteralError::Syntax { source })
    }

    /// The literal `null`.
    #[must_use]
    pub fn null() -> JsonLiteral {
        JsonLiteral(RawValue::NULL.to_owned())
    }

    /// The JSON string `text`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError::Syntax`] when the JSON reader refuses the
    /// quoted text, which a string written by [`quoted`] never makes it do.
    pub fn string(text: &str) -> Result<JsonLiteral, JsonLiteralError> {
        JsonLiteral::from_admitted_text(quoted(text))
    }

    /// The admitted JSON text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.get()
    }

    /// The value, read in place.
    #[must_use]
    pub fn piece(&self) -> JsonPiece<'_> {
        JsonPiece::new(&self.0)
    }
}

impl fmt::Display for JsonLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl FromStr for JsonLiteral {
    type Err = JsonLiteralError;

    fn from_str(text: &str) -> Result<JsonLiteral, JsonLiteralError> {
        JsonLiteral::admit(text)
    }
}

// Hand-written: two literals are equal when the JSON values they spell are, so
// the comparison ignores member order and compares numbers by value; a derive
// would compare the texts.
impl PartialEq for JsonLiteral {
    fn eq(&self, other: &JsonLiteral) -> bool {
        same_value(self.piece(), other.piece())
    }
}

impl Eq for JsonLiteral {}

// Hand-written on both halves: the text is written verbatim, and read back by
// transcribing whatever the deserializer delivers and admitting the text,
// which no derive expresses.
impl Serialize for JsonLiteral {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonLiteral {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<JsonLiteral, D::Error> {
        JsonLiteral::admit(&transcribed(deserializer)?).map_err(DeserializeError::custom)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-ngsi-ld-number", "andromeda-ngsi-ld-vocabulary"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
