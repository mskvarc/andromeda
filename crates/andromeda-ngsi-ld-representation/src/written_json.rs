//! Test fixtures: a JSON document written out as text, so that the compact
//! node reading is exercised on the text form a request arrives in.

use andromeda_json_literal::piece::JsonPiece;
use serde_json::{Value, value::RawValue};

/// One JSON document held as validated text.
pub(crate) struct WrittenJson(Box<RawValue>);

impl WrittenJson {
    /// The text of `value`, held as one JSON document.
    pub(crate) fn of(value: &Value) -> WrittenJson {
        let text = value.to_string();
        WrittenJson(RawValue::from_string(text).expect("a serialized value is valid JSON"))
    }

    /// The compact node the document reads as.
    pub(crate) fn node(&self) -> JsonPiece<'_> {
        JsonPiece::new(&self.0)
    }
}
