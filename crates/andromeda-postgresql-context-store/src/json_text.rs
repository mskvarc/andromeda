//! The JSON text of a `json` or `jsonb` column, borrowed straight from the row,
//! and the text a typed value is bound as to reach such a column.
//!
//! A stored entity document, a durable queue job or a temporal intent is read
//! by a typed streaming decoder over the column's bytes. Decoding the column
//! through `serde_json::Value` first would build and walk a whole JSON tree per
//! row only to hand it to that decoder, so the column is read as its text and
//! nothing else.
//!
//! A `json` column keeps the text it is handed. A `sqlx::types::Json` bind
//! declares its parameter `jsonb`, which the server parses into a tree and
//! prints again before a `json` column stores it, so a value is bound as the
//! text its own `Serialize` writes and its statement casts that text `::json`.

use serde::Serialize;
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

/// The JSON text of one `json` or `jsonb` column value, borrowed from the row
/// it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonText<'r>(&'r [u8]);

impl<'r> JsonText<'r> {
    /// View JSON text already held in memory as a column value would be read.
    #[must_use]
    pub const fn new(text: &'r [u8]) -> JsonText<'r> {
        JsonText(text)
    }

    /// The UTF-8 JSON text, without the binary `jsonb` format's version byte.
    #[must_use]
    pub const fn as_bytes(self) -> &'r [u8] {
        self.0
    }
}

/// Why a binary `jsonb` value carries no JSON text this reader understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JsonTextError {
    /// The value carries no version byte at all.
    #[error("a binary jsonb value is empty")]
    Empty,
    /// The value opens with a format version `PostgreSQL` has not defined.
    #[error("a binary jsonb value has format version {0}, not 1")]
    Version(u8),
}

/// The JSON text a typed value is bound as, for a column its statement casts
/// `::json`; `None` where there is no value.
///
/// # Errors
///
/// Returns the serializer's error when the value cannot be written as JSON.
pub fn optional_json_text<T: Serialize + ?Sized>(value: Option<&T>) -> Result<Option<String>, serde_json::Error> {
    value.map(serde_json::to_string).transpose()
}

/// The JSON text of a column value read in `format` from a column of type
/// `ty`: a binary `jsonb` value opens with its version byte, while every text
/// value and every `json` value is the text itself.
fn column_text<'r>(bytes: &'r [u8], format: PgValueFormat, ty: &PgTypeInfo) -> Result<&'r [u8], JsonTextError> {
    if format != PgValueFormat::Binary || *ty != <Json<()> as Type<Postgres>>::type_info() {
        return Ok(bytes);
    }
    match bytes.split_first() {
        Some((&JSONB_VERSION, text)) => Ok(text),
        Some((version, _)) => Err(JsonTextError::Version(*version)),
        None => Err(JsonTextError::Empty),
    }
}

// Hand-written because no derive maps a column type onto a borrowed byte view:
// the type is the one sqlx's own `Json` wrapper declares, so a `jsonb` and a
// `json` column are both accepted exactly where `Json` would be.
impl Type<Postgres> for JsonText<'_> {
    fn type_info() -> PgTypeInfo {
        <Json<()> as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <Json<()> as Type<Postgres>>::compatible(ty)
    }
}

// Hand-written for the same reason: the decode is a view over the row's bytes
// with the binary `jsonb` format's version byte stripped, which no derive
// expresses.
impl<'r> Decode<'r, Postgres> for JsonText<'r> {
    fn decode(value: PgValueRef<'r>) -> Result<JsonText<'r>, BoxDynError> {
        let bytes = value.as_bytes()?;
        Ok(JsonText(column_text(bytes, value.format(), &value.type_info())?))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::json_text::{JsonText, JsonTextError, column_text, optional_json_text};
    use pretty_assertions::assert_eq;
    use sqlx::{
        Postgres,
        Type,
        postgres::{PgTypeInfo, PgValueFormat, types::Oid},
        types::Json,
    };

    /// The `pg_type` OID of `json`, fixed by the PostgreSQL catalogue.
    const JSON_OID: u32 = 114;

    #[test]
    fn the_text_view_accepts_the_columns_the_json_wrapper_accepts() {
        assert_eq!(<JsonText<'_> as Type<Postgres>>::type_info(), <Json<()> as Type<Postgres>>::type_info());
        assert!(<JsonText<'_> as Type<Postgres>>::compatible(&<Json<()> as Type<Postgres>>::type_info()));
        assert!(<JsonText<'_> as Type<Postgres>>::compatible(&PgTypeInfo::with_oid(Oid(JSON_OID))));
        assert!(!<JsonText<'_> as Type<Postgres>>::compatible(&<String as Type<Postgres>>::type_info()));
    }

    #[test]
    fn the_text_view_hands_back_the_bytes_it_borrows() {
        let text = br#"[{"@id":"urn:ngsi-ld:A"}]"#;
        assert_eq!(JsonText(text).as_bytes(), text);
    }

    #[test]
    fn an_unknown_format_version_names_the_version_it_found() {
        assert_eq!(JsonTextError::Version(2).to_string(), "a binary jsonb value has format version 2, not 1");
    }

    /// `json_send` writes a `json` value's text as it is, with no version byte,
    /// so a leading byte is never stripped from it.
    #[test]
    fn a_json_column_hands_back_its_text_unchanged() {
        let text = br#"{"a":1}"#;
        let read = column_text(text, PgValueFormat::Binary, &PgTypeInfo::with_oid(Oid(JSON_OID)));
        assert_eq!(read, Ok(&text[..]));
    }

    #[test]
    fn a_binary_jsonb_value_loses_only_its_version_byte() {
        let read = column_text(b"\x01{\"a\":1}", PgValueFormat::Binary, &<Json<()> as Type<Postgres>>::type_info());
        assert_eq!(read, Ok(&br#"{"a":1}"#[..]));
    }

    #[test]
    fn a_value_is_bound_as_the_text_its_serializer_writes() {
        assert_eq!(optional_json_text(Some(&[1, 2])).expect("serialises"), Some("[1,2]".to_owned()));
        assert_eq!(optional_json_text::<[u8]>(None).expect("nothing to serialise"), None);
    }
}
