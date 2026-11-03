//! A value of JSON text read as a compact node.
//!
//! Every value is a slice of the text it was written in, so reading a node
//! parses only its own level, and a value the model keeps as JSON is admitted
//! straight from its slice.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape, decoded_string};
use andromeda_json_literal::{
    JsonLiteral,
    admission::JsonLiteralError,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use serde::de::DeserializeSeed;
use serde_json::Deserializer as JsonReader;
use std::{borrow::Cow, convert::Infallible};

impl<'n> CompactNode<'n> for JsonPiece<'n> {
    type Unreadable = Infallible;

    fn shape(self) -> NodeShape<'n, JsonPiece<'n>> {
        // A piece is the text of one JSON value, which always reads as the
        // value its first character announces; the reader cannot fail on it,
        // so its empty answer is never the one taken.
        match self.kind() {
            JsonKind::Null => NodeShape::Null,
            JsonKind::Boolean => NodeShape::Boolean(self.as_bool().unwrap_or_default()),
            JsonKind::Number => NodeShape::Number(Cow::Borrowed(self.text().trim())),
            JsonKind::String => NodeShape::String(self.as_str().unwrap_or(Cow::Borrowed(""))),
            JsonKind::Array => NodeShape::Array(self.elements().unwrap_or_default()),
            JsonKind::Object => NodeShape::Object(NodeMembers::new(self.members().unwrap_or_default())),
        }
    }

    fn is_null(self) -> bool {
        self.kind() == JsonKind::Null
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, Infallible> {
        Ok(self.to_literal())
    }

    fn decoded_by<T, S: for<'de> DeserializeSeed<'de, Value = T>>(self, seed: S) -> Result<Result<T, serde_json::Error>, Infallible> {
        // Only the kind is looked at: the shape of an array or an object holds
        // a node per item, and held across the decode it would add the size of
        // the value to the size of what it decodes into.
        Ok(match self.kind() {
            JsonKind::String => decoded_string(&self.as_str().unwrap_or(Cow::Borrowed("")), seed),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::Array | JsonKind::Object => {
                let mut reader = JsonReader::from_str(self.text());
                seed.deserialize(&mut reader).and_then(|read| reader.end().map(|()| read))
            }
        })
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, Infallible> {
        Ok(serde_json::from_str(self.text()))
    }
}
