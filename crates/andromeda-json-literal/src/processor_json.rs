//! Handing a literal to the JSON-LD processor and reading one back.
//!
//! The processor holds an `@json` value in its own JSON type; that type is
//! built here from the literal's text and printed back into one, and never
//! held anywhere else.

use crate::{JsonLiteral, admission::JsonLiteralError};
use jstrict::{Print, Value, parse::parse_str_value};

impl JsonLiteral {
    /// The literal as the JSON-LD processor holds an `@json` value.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError::ProcessorText`] when the processor's reader
    /// refuses the admitted text.
    pub fn to_processor_json(&self) -> Result<Value, JsonLiteralError> {
        parse_str_value(self.text()).map_err(|source| JsonLiteralError::ProcessorText { source })
    }

    /// Admit the `@json` value the JSON-LD processor holds.
    ///
    /// # Errors
    ///
    /// As [`JsonLiteral::admit`].
    pub fn from_processor_json(value: &Value) -> Result<JsonLiteral, JsonLiteralError> {
        JsonLiteral::admit(&value.compact_print().to_string())
    }
}
