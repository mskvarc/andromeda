//! The JSON text of a `jsonb` column, borrowed straight from the row.
//!
//! A stored entity document, a durable queue job or a temporal intent is read
//! by a typed streaming decoder over the column's bytes. Decoding the column
//! through `serde_json::Value` first would build and walk a whole JSON tree per
//! row only to hand it to that decoder, so the column is read as its text and
//! nothing else.

use sqlx::{
    Decode,
    Postgres,
    Type,
    ValueRef,
    error::BoxDynError,
    postgres::{PgTypeInfo, PgValueFormat, PgValueRef},
    types::Json,
};
use thiserror::Error;

/// The version byte that opens every binary `jsonb` value. `PostgreSQL` has
/// defined only version 1 of that layout.
const JSONB_VERSION: u8 = 0x01;

/// The JSON text of one `jsonb` (or `json`) column value, borrowed from the row
/// it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonbText<'r>(&'r [u8]);

impl<'r> JsonbText<'r> {
    /// View JSON text already held in memory as a column value would be read.
    #[must_use]
    pub const fn new(text: &'r [u8]) -> JsonbText<'r> {
        JsonbText(text)
    }

    /// The UTF-8 JSON text, without the binary format's version byte.
    #[must_use]
    pub const fn as_bytes(self) -> &'r [u8] {
        self.0
    }
}

/// Why a binary `jsonb` value carries no JSON text this reader understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JsonbTextError {
    /// The value carries no version byte at all.
    #[error("a binary jsonb value is empty")]
    Empty,
    /// The value opens with a format version `PostgreSQL` has not defined.
    #[error("a binary jsonb value has format version {0}, not 1")]
    Version(u8),
}

// Hand-written because no derive maps a column type onto a borrowed byte view:
// the type is the one sqlx's own `Json` wrapper declares, so a `jsonb` and a
// `json` column are both accepted exactly where `Json` would be.
impl Type<Postgres> for JsonbText<'_> {
    fn type_info() -> PgTypeInfo {
        <Json<()> as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <Json<()> as Type<Postgres>>::compatible(ty)
    }
}

// Hand-written for the same reason: the decode is a view over the row's bytes
// with the binary format's version byte stripped, which no derive expresses.
impl<'r> Decode<'r, Postgres> for JsonbText<'r> {
    fn decode(value: PgValueRef<'r>) -> Result<JsonbText<'r>, BoxDynError> {
        let binary_jsonb = value.format() == PgValueFormat::Binary && *value.type_info() == <Json<()> as Type<Postgres>>::type_info();
        let bytes = value.as_bytes()?;
        if !binary_jsonb {
            return Ok(JsonbText(bytes));
        }
        match bytes.split_first() {
            Some((&JSONB_VERSION, text)) => Ok(JsonbText(text)),
            Some((version, _)) => Err(Box::new(JsonbTextError::Version(*version))),
            None => Err(Box::new(JsonbTextError::Empty)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonb_text::{JsonbText, JsonbTextError};
    use pretty_assertions::assert_eq;
    use sqlx::{Postgres, Type, types::Json};

    #[test]
    fn the_text_view_accepts_the_columns_the_json_wrapper_accepts() {
        assert_eq!(<JsonbText<'_> as Type<Postgres>>::type_info(), <Json<()> as Type<Postgres>>::type_info());
        assert!(<JsonbText<'_> as Type<Postgres>>::compatible(&<Json<()> as Type<Postgres>>::type_info()));
        assert!(!<JsonbText<'_> as Type<Postgres>>::compatible(&<String as Type<Postgres>>::type_info()));
    }

    #[test]
    fn the_text_view_hands_back_the_bytes_it_borrows() {
        let text = br#"[{"@id":"urn:ngsi-ld:A"}]"#;
        assert_eq!(JsonbText(text).as_bytes(), text);
    }

    #[test]
    fn an_unknown_format_version_names_the_version_it_found() {
        assert_eq!(JsonbTextError::Version(2).to_string(), "a binary jsonb value has format version 2, not 1");
    }
}
